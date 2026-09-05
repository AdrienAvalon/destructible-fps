//! Bounded small-deflection 3D beam equilibrium for immutable server structural jobs.
//!
//! Six DOFs per node: translations in metres and rotations in radians. This module never mutates
//! voxels or replicas. Converged forces are evidence for a revalidated server fracture transaction,
//! not permission to change gameplay. See docs/structural-elasticity.md for model limitations.

use crate::IVec3;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ELASTIC_NODES: usize = 4_096;
pub const MAX_ELASTIC_ITERATIONS: usize = 512;
pub const MAX_ELASTIC_STEP: usize = 32;
type Dofs = [f64; 6];

#[derive(Clone, Copy, Debug)]
pub struct ElasticNode {
    pub position: IVec3,
    pub young_modulus_pa: f64,
    pub poisson_ratio: f64,
    pub mass_kg: f64,
    /// Occupied damaged voxels keep their mass; integrity changes section stiffness, not density.
    pub integrity: u8,
    pub fixed: bool,
    /// External forces in N, then moments in N.m, in world axes. Gravity is added separately.
    pub load: Dofs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElasticError {
    InvalidInput,
    TooManyNodes,
    NonCanonicalNodes,
    UnanchoredComponent,
    InvalidOptions,
    InvalidStepBudget,
    NumericalBreakdown,
    IterationLimit,
    OutsideLinearRegime,
    NotConverged,
}

impl core::fmt::Display for ElasticError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "elastic equilibrium: {self:?}")
    }
}
impl std::error::Error for ElasticError {}

#[derive(Clone, Copy, Debug)]
pub struct ElasticOptions {
    pub max_iterations: usize,
    pub relative_tolerance: f64,
    pub absolute_tolerance_n: f64,
}

impl Default for ElasticOptions {
    fn default() -> Self {
        Self {
            max_iterations: MAX_ELASTIC_ITERATIONS,
            relative_tolerance: 1e-7,
            absolute_tolerance_n: 1e-5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElasticProgress {
    Pending,
    Converged,
}

#[derive(Clone, Copy, Debug)]
struct Beam {
    ends: [usize; 2],
    axis: usize,
    length: f64,
    axial: f64,
    torsion: f64,
    bending: f64,
    shear_parameter: f64,
}

impl Beam {
    fn new(ends: [usize; 2], axis: usize, nodes: &[ElasticNode], length: f64) -> Self {
        let [left, right] = ends.map(|index| nodes[index]);
        let young = harmonic(left.young_modulus_pa, right.young_modulus_pa);
        let shear = harmonic(
            left.young_modulus_pa / (2.0 * (1.0 + left.poisson_ratio)),
            right.young_modulus_pa / (2.0 * (1.0 + right.poisson_ratio)),
        );
        let fraction = f64::from(left.integrity.min(right.integrity)) / 255.0;
        let area = length.powi(2) * fraction;
        let inertia = length.powi(4) * fraction.powi(2) / 12.0;
        // Closed-form two-node Timoshenko stiffness, square-section shear correction kappa=5/6.
        // See numgeo's beam formulation linked in docs/structural-elasticity.md.
        let shear_parameter =
            12.0 * young * inertia / ((5.0 / 6.0) * shear * area * length.powi(2));
        Self {
            ends,
            axis,
            length,
            axial: young * area / length,
            // Saint-Venant square torsion constant, not the polar area moment 2I.
            torsion: shear * (0.1406 * length.powi(4) * fraction.powi(2)) / length,
            bending: young * inertia / (length.powi(3) * (1.0 + shear_parameter)),
            shear_parameter,
        }
    }

    /// Full symmetric Timoshenko element, including shear and translation/rotation coupling.
    fn forces(self, left: Dofs, right: Dofs) -> [Dofs; 2] {
        let mut output = [[0.0; 6]; 2];
        for (dof, stiffness) in [(self.axis, self.axial), (self.axis + 3, self.torsion)] {
            let force = stiffness * (left[dof] - right[dof]);
            output[0][dof] += force;
            output[1][dof] -= force;
        }
        for transverse in 0..3 {
            if transverse == self.axis {
                continue;
            }
            let rotation = 3 - self.axis - transverse;
            let sign = if (self.axis + 1) % 3 == transverse {
                1.0
            } else {
                -1.0
            };
            let delta = left[transverse] - right[transverse];
            let first_angle = sign * left[rotation + 3];
            let last_angle = sign * right[rotation + 3];
            let shear =
                self.bending * (6.0 * self.length).mul_add(first_angle + last_angle, 12.0 * delta);
            let moment_left = self.bending
                * self.length.powi(2).mul_add(
                    (2.0 - self.shear_parameter)
                        .mul_add(last_angle, (4.0 + self.shear_parameter) * first_angle),
                    6.0 * self.length * delta,
                );
            let moment_right = self.bending
                * self.length.powi(2).mul_add(
                    (4.0 + self.shear_parameter)
                        .mul_add(last_angle, (2.0 - self.shear_parameter) * first_angle),
                    6.0 * self.length * delta,
                );
            output[0][transverse] += shear;
            output[1][transverse] -= shear;
            output[0][rotation + 3] += sign * moment_left;
            output[1][rotation + 3] += sign * moment_right;
        }
        output
    }

    fn diagonal(self) -> Dofs {
        let mut diagonal = [0.0; 6];
        diagonal[self.axis] = self.axial;
        diagonal[self.axis + 3] = self.torsion;
        for transverse in 0..3 {
            if transverse != self.axis {
                diagonal[transverse] = 12.0 * self.bending;
                diagonal[transverse + 3] =
                    (4.0 + self.shear_parameter) * self.length.powi(2) * self.bending;
            }
        }
        diagonal
    }
}

pub struct ElasticModel {
    nodes: Vec<ElasticNode>,
    beams: Vec<Beam>,
    length: f64,
    loads: Vec<Dofs>,
    diagonal: Vec<Dofs>,
}

impl ElasticModel {
    /// Builds only the supplied complete mechanical domain, joining cardinal neighbour centres.
    /// The caller owns domain extraction; omitted world geometry is NOT inferred to be anchored.
    ///
    /// # Errors
    /// Rejects nonfinite/out-of-range inputs, noncanonical ordering, more than 4096 nodes, and any
    /// component without a clamped node. No floating component is silently regularized.
    pub fn new(
        nodes: &[ElasticNode],
        length: f64,
        gravity: [f64; 3],
    ) -> Result<Self, ElasticError> {
        if nodes.len() > MAX_ELASTIC_NODES {
            return Err(ElasticError::TooManyNodes);
        }
        if nodes.is_empty()
            || !in_range(length, 0.01, 10.0)
            || gravity.iter().any(|value| !in_range(*value, -100.0, 100.0))
            || nodes.iter().any(|node| !valid_node(*node))
        {
            return Err(ElasticError::InvalidInput);
        }
        if nodes
            .windows(2)
            .any(|pair| pair[0].position >= pair[1].position)
        {
            return Err(ElasticError::NonCanonicalNodes);
        }
        let lookup: BTreeMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.position, i))
            .collect();
        let mut parents: Vec<_> = (0..nodes.len()).collect();
        let mut beams = Vec::with_capacity(nodes.len() * 3);
        let mut diagonal = vec![[0.0; 6]; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            for axis in 0..3 {
                let mut coordinates = [node.position.x, node.position.y, node.position.z];
                let Some(next) = coordinates[axis].checked_add(1) else {
                    continue;
                };
                coordinates[axis] = next;
                let position = IVec3::new(coordinates[0], coordinates[1], coordinates[2]);
                if let Some(&other) = lookup.get(&position) {
                    let beam = Beam::new([index, other], axis, nodes, length);
                    for end in beam.ends {
                        for (target, value) in diagonal[end].iter_mut().zip(beam.diagonal()) {
                            *target += value;
                        }
                    }
                    let first_root = root(&mut parents, index);
                    let other_root = root(&mut parents, other);
                    parents[other_root] = first_root;
                    beams.push(beam);
                }
            }
        }
        let anchored: BTreeSet<_> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.fixed)
            .map(|(i, _)| root(&mut parents, i))
            .collect();
        if (0..nodes.len()).any(|i| !anchored.contains(&root(&mut parents, i))) {
            return Err(ElasticError::UnanchoredComponent);
        }
        let loads = nodes
            .iter()
            .map(|node| {
                let mut load = node.load;
                for axis in 0..3 {
                    load[axis] = node.mass_kg.mul_add(gravity[axis], load[axis]);
                }
                load
            })
            .collect();
        // Scaled rotations q=L*theta make all solver DOFs lengths and all residuals forces.
        for (node, values) in nodes.iter().zip(&mut diagonal) {
            for (dof, value) in values.iter_mut().enumerate() {
                if node.fixed {
                    *value = 1.0;
                } else if dof >= 3 {
                    *value /= length.powi(2);
                }
                if !value.is_finite() || *value <= 0.0 {
                    return Err(ElasticError::NumericalBreakdown);
                }
            }
        }
        Ok(Self {
            nodes: nodes.to_vec(),
            beams,
            length,
            loads,
            diagonal,
        })
    }

    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }
    #[must_use]
    pub const fn beam_count(&self) -> usize {
        self.beams.len()
    }

    fn apply(&self, scaled: &[Dofs], output: &mut [Dofs]) {
        output.fill([0.0; 6]);
        for beam in &self.beams {
            let displacements = beam.ends.map(|index| physical(scaled[index], self.length));
            let forces = beam.forces(displacements[0], displacements[1]);
            for (index, force) in beam.ends.into_iter().zip(forces) {
                for (dof, value) in force.into_iter().enumerate() {
                    output[index][dof] += if dof < 3 { value } else { value / self.length };
                }
            }
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if node.fixed {
                output[index] = scaled[index];
            }
        }
    }
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn valid_node(node: ElasticNode) -> bool {
    in_range(node.young_modulus_pa, 1e4, 1e12)
        && in_range(node.poisson_ratio, -0.9, 0.49)
        && in_range(node.mass_kg, 0.0, 1e9)
        && node.integrity > 0
        && node.load.iter().all(|value| in_range(*value, -1e12, 1e12))
}
fn in_range(value: f64, minimum: f64, maximum: f64) -> bool {
    value.is_finite() && (minimum..=maximum).contains(&value)
}
fn harmonic(left: f64, right: f64) -> f64 {
    2.0 * left * right / (left + right)
}
fn physical(mut scaled: Dofs, length: f64) -> Dofs {
    for value in &mut scaled[3..] {
        *value /= length;
    }
    scaled
}
fn dot(left: &[Dofs], right: &[Dofs]) -> f64 {
    left.iter()
        .flatten()
        .zip(right.iter().flatten())
        .map(|(a, b)| a * b)
        .sum()
}

pub struct ElasticJob {
    model: ElasticModel,
    options: ElasticOptions,
    displacement: Vec<Dofs>,
    residual: Vec<Dofs>,
    direction: Vec<Dofs>,
    product: Vec<Dofs>,
    rhs: Vec<Dofs>,
    rho: f64,
    threshold: f64,
    initial_norm: f64,
    iterations: usize,
    converged: bool,
    failed: Option<ElasticError>,
}

#[derive(Debug)]
pub struct ElasticSolution {
    pub positions: Vec<IVec3>,
    pub bond_ends: Vec<[usize; 2]>,
    pub displacements: Vec<Dofs>,
    pub reactions: Vec<Dofs>,
    pub bond_end_forces: Vec<[Dofs; 2]>,
    pub iterations: usize,
    pub residual_n: f64,
    pub relative_residual: f64,
}

impl ElasticJob {
    /// # Errors
    /// Rejects invalid tolerances or a global iteration budget above the hard maximum.
    pub fn new(model: ElasticModel, options: ElasticOptions) -> Result<Self, ElasticError> {
        if !(1..=MAX_ELASTIC_ITERATIONS).contains(&options.max_iterations)
            || !in_range(options.relative_tolerance, 1e-12, 1e-3)
            || !in_range(options.absolute_tolerance_n, 1e-12, 1.0)
        {
            return Err(ElasticError::InvalidOptions);
        }
        let rhs: Vec<_> = model
            .loads
            .iter()
            .zip(&model.nodes)
            .map(|(&load, node)| {
                if node.fixed {
                    [0.0; 6]
                } else {
                    physical(load, model.length)
                }
            })
            .collect();
        let initial_norm = dot(&rhs, &rhs).sqrt();
        let threshold = options
            .absolute_tolerance_n
            .max(options.relative_tolerance * initial_norm);
        let direction: Vec<Dofs> = rhs
            .iter()
            .zip(&model.diagonal)
            .map(|(load, diagonal)| core::array::from_fn(|i| load[i] / diagonal[i]))
            .collect();
        let rho = dot(&rhs, &direction);
        let count = model.nodes.len();
        Ok(Self {
            model,
            options,
            displacement: vec![[0.0; 6]; count],
            residual: rhs.clone(),
            direction,
            product: vec![[0.0; 6]; count],
            rhs,
            rho,
            threshold,
            initial_norm,
            iterations: 0,
            converged: initial_norm <= threshold,
            failed: None,
        })
    }

    /// Advances at most the requested number of PCG iterations, with no allocation or world access.
    ///
    /// # Errors
    /// Invalid per-call budgets leave the job unchanged. Numerical or total-budget failure latches;
    /// no partially converged displacement is ever exposed as an authoritative result.
    pub fn advance(&mut self, iterations: usize) -> Result<ElasticProgress, ElasticError> {
        if !(1..=MAX_ELASTIC_STEP).contains(&iterations) {
            return Err(ElasticError::InvalidStepBudget);
        }
        if let Some(error) = self.failed {
            return Err(error);
        }
        for _ in 0..iterations {
            if self.converged {
                return Ok(ElasticProgress::Converged);
            }
            if let Err(error) = self.iterate() {
                self.failed = Some(error);
                return Err(error);
            }
        }
        Ok(if self.converged {
            ElasticProgress::Converged
        } else {
            ElasticProgress::Pending
        })
    }

    fn iterate(&mut self) -> Result<(), ElasticError> {
        self.model.apply(&self.direction, &mut self.product);
        let denominator = dot(&self.direction, &self.product);
        if !denominator.is_finite()
            || denominator <= 0.0
            || !self.rho.is_finite()
            || self.rho <= 0.0
        {
            return Err(ElasticError::NumericalBreakdown);
        }
        let alpha = self.rho / denominator;
        for index in 0..self.displacement.len() {
            for dof in 0..6 {
                self.displacement[index][dof] =
                    alpha.mul_add(self.direction[index][dof], self.displacement[index][dof]);
                self.residual[index][dof] =
                    alpha.mul_add(-self.product[index][dof], self.residual[index][dof]);
            }
        }
        self.iterations += 1;
        let recursive_norm = dot(&self.residual, &self.residual).sqrt();
        if !recursive_norm.is_finite() {
            return Err(ElasticError::NumericalBreakdown);
        }
        // Always verify actual equilibrium before accepting. Restart only on a false convergence
        // candidate: unconditional short restarts discard long-range beam conjugacy.
        let refresh =
            recursive_norm <= self.threshold || self.iterations == self.options.max_iterations;
        if refresh {
            self.model.apply(&self.displacement, &mut self.product);
            for index in 0..self.residual.len() {
                for dof in 0..6 {
                    self.residual[index][dof] = self.rhs[index][dof] - self.product[index][dof];
                }
            }
            let actual_norm = dot(&self.residual, &self.residual).sqrt();
            if !actual_norm.is_finite() {
                return Err(ElasticError::NumericalBreakdown);
            }
            if actual_norm <= self.threshold {
                self.converged = true;
                return self.validate_linear_range();
            }
        }
        if self.iterations == self.options.max_iterations {
            return Err(ElasticError::IterationLimit);
        }
        let mut next_rho = 0.0;
        for (residual, diagonal) in self.residual.iter().zip(&self.model.diagonal) {
            for dof in 0..6 {
                next_rho += residual[dof] * residual[dof] / diagonal[dof];
            }
        }
        if !next_rho.is_finite() || next_rho <= 0.0 {
            return Err(ElasticError::NumericalBreakdown);
        }
        let beta = if refresh { 0.0 } else { next_rho / self.rho };
        for index in 0..self.direction.len() {
            for dof in 0..6 {
                self.direction[index][dof] = self.residual[index][dof]
                    / self.model.diagonal[index][dof]
                    + beta * self.direction[index][dof];
            }
        }
        self.rho = next_rho;
        Ok(())
    }

    fn validate_linear_range(&self) -> Result<(), ElasticError> {
        if self.displacement.iter().any(|dofs| {
            dofs.iter().any(|v| !v.is_finite())
                || dofs[3..].iter().any(|v| v.abs() / self.model.length > 0.1)
        }) {
            return Err(ElasticError::OutsideLinearRegime);
        }
        for beam in &self.model.beams {
            if (0..3).any(|axis| {
                (self.displacement[beam.ends[0]][axis] - self.displacement[beam.ends[1]][axis])
                    .abs()
                    / self.model.length
                    > 0.1
            }) {
                return Err(ElasticError::OutsideLinearRegime);
            }
        }
        Ok(())
    }

    /// Consumes a converged job; fixed-node reactions include external loads applied to the anchors.
    ///
    /// # Errors
    /// A failed or incomplete solve cannot be used as a result.
    pub fn finish(self) -> Result<ElasticSolution, ElasticError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        if !self.converged {
            return Err(ElasticError::NotConverged);
        }
        let displacements: Vec<_> = self
            .displacement
            .iter()
            .map(|&v| physical(v, self.model.length))
            .collect();
        let mut reactions = vec![[0.0; 6]; self.model.nodes.len()];
        let bond_end_forces: Vec<_> = self
            .model
            .beams
            .iter()
            .map(|beam| {
                let forces = beam.forces(displacements[beam.ends[0]], displacements[beam.ends[1]]);
                for (end, force) in beam.ends.into_iter().zip(forces) {
                    for dof in 0..6 {
                        reactions[end][dof] += force[dof];
                    }
                }
                forces
            })
            .collect();
        for (index, node) in self.model.nodes.iter().enumerate() {
            for (dof, reaction) in reactions[index].iter_mut().enumerate() {
                *reaction = if node.fixed {
                    *reaction - self.model.loads[index][dof]
                } else {
                    0.0
                };
            }
        }
        let residual_n = dot(&self.residual, &self.residual).sqrt();
        Ok(ElasticSolution {
            positions: self.model.nodes.iter().map(|node| node.position).collect(),
            bond_ends: self.model.beams.iter().map(|beam| beam.ends).collect(),
            displacements,
            reactions,
            bond_end_forces,
            iterations: self.iterations,
            residual_n,
            relative_residual: if self.initial_norm > 0.0 {
                residual_n / self.initial_norm
            } else {
                0.0
            },
        })
    }
}

#[cfg(test)]
mod tests;
