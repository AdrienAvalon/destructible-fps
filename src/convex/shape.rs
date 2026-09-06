//! Canonical, closed convex material boundaries for the explicitly typed inspection scene.
//! Integer planes, query edges and rendered faces derive from this one validated source.
use super::ConvexError;
use crate::{IVec3, Voxel, world::query::PhysicalBox};

pub const MAX_CONVEX_VERTICES: usize = 32;
pub const MAX_CONVEX_FACES: usize = 16;
pub const MAX_CONVEX_FACE_VERTICES: usize = 8;
pub const MAX_CONVEX_EDGES: usize = 64;
/// Local coordinates in 1/256 metre: at most eight metres on each axis.
pub const MAX_CONVEX_COORDINATE: u16 = 2_048;
/// Shared inspection/query/render domain; every q256 vertex remains exactly representable in f32.
pub const MAX_CONVEX_ORIGIN: i32 = 16_384;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvexFaceInput {
    pub indices: Vec<u8>,
    pub cut: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConvexFace {
    indices: Vec<u8>,
    cut: bool,
}
impl ConvexFace {
    #[must_use]
    pub fn indices(&self) -> &[u8] {
        &self.indices
    }
    #[must_use]
    pub const fn cut(&self) -> bool {
        self.cut
    }
}

/// Outward integer plane in local lattice coordinates: inside is `normal dot q <= offset`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConvexPlane {
    normal: [i64; 3],
    offset: i64,
}
impl ConvexPlane {
    #[must_use]
    pub const fn normal(self) -> [i64; 3] {
        self.normal
    }
    #[must_use]
    pub const fn offset(self) -> i64 {
        self.offset
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvexFragment {
    origin: IVec3,
    vertices: Vec<[u16; 3]>,
    faces: Vec<ConvexFace>,
    planes: Vec<ConvexPlane>,
    edges: Vec<[u8; 2]>,
    material: Voxel,
    bounds: PhysicalBox,
    fingerprint: u128,
}
impl ConvexFragment {
    /// Validate before publication; canonicalization changes indices, never positions or winding.
    /// # Errors
    /// Rejects bounds/budgets, air, invalid topology, inward/nonconvex/nonplanar/degenerate faces,
    /// unused vertices and zero volume. Allocation failures discard the complete candidate.
    pub fn new(
        origin: IVec3,
        vertices: &[[u16; 3]],
        faces: &[ConvexFaceInput],
        material: Voxel,
    ) -> Result<Self, ConvexError> {
        validate_input_bounds(origin, vertices, faces, material)?;
        let (vertices, remap) = canonical_vertices(vertices)?;
        let faces = canonical_faces(faces, &remap)?;
        let planes = validate_faces(&vertices, &faces)?;
        let edges = validate_topology(&vertices, &faces)?;
        let bounds = physical_bounds(origin, &vertices)?;
        let fingerprint = fingerprint(origin, &vertices, &faces, material)?;
        Ok(Self {
            origin,
            vertices,
            faces,
            planes,
            edges,
            material,
            bounds,
            fingerprint,
        })
    }
    #[must_use]
    pub fn vertices(&self) -> &[[u16; 3]] {
        &self.vertices
    }
    #[must_use]
    pub fn faces(&self) -> &[ConvexFace] {
        &self.faces
    }
    #[must_use]
    pub fn planes(&self) -> &[ConvexPlane] {
        &self.planes
    }
    #[must_use]
    pub fn edges(&self) -> &[[u8; 2]] {
        &self.edges
    }
    #[must_use]
    pub const fn origin(&self) -> IVec3 {
        self.origin
    }
    #[must_use]
    pub const fn material(&self) -> Voxel {
        self.material
    }
    #[must_use]
    pub const fn bounds(&self) -> PhysicalBox {
        self.bounds
    }
    /// Deterministic cache/regression identity, not cryptographic authentication.
    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
}

fn reserved<T>(length: usize) -> Result<Vec<T>, ConvexError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(length)
        .map_err(|_| ConvexError::Budget)?;
    Ok(result)
}

fn validate_input_bounds(
    origin: IVec3,
    vertices: &[[u16; 3]],
    faces: &[ConvexFaceInput],
    material: Voxel,
) -> Result<(), ConvexError> {
    if vertices.len() > MAX_CONVEX_VERTICES
        || faces.len() > MAX_CONVEX_FACES
        || faces
            .iter()
            .any(|face| face.indices.len() > MAX_CONVEX_FACE_VERTICES)
    {
        return Err(ConvexError::Budget);
    }
    if vertices.len() < 4
        || faces.len() < 4
        || !material.is_solid()
        || faces.iter().any(|face| face.indices.len() < 3)
    {
        return Err(ConvexError::InvalidShape);
    }
    if [origin.x, origin.y, origin.z]
        .into_iter()
        .any(|v| v.unsigned_abs() > MAX_CONVEX_ORIGIN.cast_unsigned())
        || vertices
            .iter()
            .flatten()
            .any(|&v| v > MAX_CONVEX_COORDINATE)
    {
        return Err(ConvexError::Bounds);
    }
    Ok(())
}

type CanonicalVertices = (Vec<[u16; 3]>, Vec<u8>);
fn canonical_vertices(input: &[[u16; 3]]) -> Result<CanonicalVertices, ConvexError> {
    let mut vertices = reserved(input.len())?;
    vertices.extend_from_slice(input);
    vertices.sort_unstable();
    if vertices.windows(2).any(|v| v[0] == v[1]) {
        return Err(ConvexError::InvalidShape);
    }
    let mut remap = reserved(input.len())?;
    for vertex in input {
        let index = vertices
            .binary_search(vertex)
            .map_err(|_| ConvexError::InvalidShape)?;
        remap.push(u8::try_from(index).map_err(|_| ConvexError::Budget)?);
    }
    Ok((vertices, remap))
}

fn canonical_faces(
    input: &[ConvexFaceInput],
    remap: &[u8],
) -> Result<Vec<ConvexFace>, ConvexError> {
    let mut faces = reserved(input.len())?;
    let mut used = [false; MAX_CONVEX_VERTICES];
    for face in input {
        let mut indices = reserved(face.indices.len())?;
        for &index in &face.indices {
            let canonical = *remap
                .get(usize::from(index))
                .ok_or(ConvexError::InvalidShape)?;
            if indices.contains(&canonical) {
                return Err(ConvexError::InvalidShape);
            }
            indices.push(canonical);
            used[usize::from(canonical)] = true;
        }
        let first = indices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| **v)
            .map(|(i, _)| i)
            .ok_or(ConvexError::InvalidShape)?;
        indices.rotate_left(first);
        faces.push(ConvexFace {
            indices,
            cut: face.cut,
        });
    }
    if used[..remap.len()].iter().any(|&v| !v) {
        return Err(ConvexError::InvalidShape);
    }
    faces.sort_unstable();
    if faces.windows(2).any(|f| f[0].indices == f[1].indices) {
        return Err(ConvexError::InvalidShape);
    }
    Ok(faces)
}

fn point(vertex: [u16; 3]) -> [i64; 3] {
    vertex.map(i64::from)
}
fn subtract(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    std::array::from_fn(|axis| a[axis] - b[axis])
}
const fn cross(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
const fn dot(a: [i64; 3], b: [i64; 3]) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
const fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

fn face_plane(vertices: &[[u16; 3]], face: &ConvexFace) -> Result<ConvexPlane, ConvexError> {
    let a = point(vertices[usize::from(face.indices[0])]);
    let b = point(vertices[usize::from(face.indices[1])]);
    let c = point(vertices[usize::from(face.indices[2])]);
    let normal = cross(subtract(b, a), subtract(c, a));
    let divisor = normal.into_iter().fold(0, |value, n| gcd(value, n.abs()));
    if divisor == 0 {
        return Err(ConvexError::InvalidShape);
    }
    let normal = normal.map(|n| n / divisor);
    let offset = dot(normal, a);
    let mut strictly_inside = false;
    for (index, vertex) in vertices.iter().enumerate() {
        let distance = dot(normal, point(*vertex)) - offset;
        if distance > 0
            || (face
                .indices
                .contains(&u8::try_from(index).map_err(|_| ConvexError::Budget)?)
                && distance != 0)
        {
            return Err(ConvexError::InvalidShape);
        }
        strictly_inside |= distance < 0;
    }
    if !strictly_inside {
        return Err(ConvexError::InvalidShape);
    }
    // Each directed polygon edge must have ALL other face vertices strictly to its left.
    // This rejects star orderings, concavity and collinear corners, not just local wrong turns.
    for index in 0..face.indices.len() {
        let start = face.indices[index];
        let end = face.indices[(index + 1) % face.indices.len()];
        let a = point(vertices[usize::from(start)]);
        let edge = subtract(point(vertices[usize::from(end)]), a);
        for &other in &face.indices {
            if other != start
                && other != end
                && dot(
                    normal,
                    cross(edge, subtract(point(vertices[usize::from(other)]), a)),
                ) <= 0
            {
                return Err(ConvexError::InvalidShape);
            }
        }
    }
    Ok(ConvexPlane { normal, offset })
}

fn validate_faces(
    vertices: &[[u16; 3]],
    faces: &[ConvexFace],
) -> Result<Vec<ConvexPlane>, ConvexError> {
    // All arithmetic above follows validated |edge| <= 2048, |raw normal| <= 2*2048².
    // The largest n dot (edge cross edge) is <= 3*(2*2048²)² < 2^48; i64 is sufficient.
    let mut planes = reserved(faces.len())?;
    let mut signed_volume = 0_i64;
    for face in faces {
        let plane = face_plane(vertices, face)?;
        // One complete polygon per supporting plane: duplicate or separately triangulated
        // coplanar faces are not a second surface in this canonical source format.
        if planes.contains(&plane) {
            return Err(ConvexError::InvalidShape);
        }
        planes.push(plane);
        let a = point(vertices[usize::from(face.indices[0])]);
        for triangle in face.indices[1..].windows(2) {
            let b = point(vertices[usize::from(triangle[0])]);
            let c = point(vertices[usize::from(triangle[1])]);
            signed_volume = signed_volume
                .checked_add(dot(a, cross(b, c)))
                .ok_or(ConvexError::Arithmetic)?;
        }
    }
    if signed_volume <= 0 {
        return Err(ConvexError::InvalidShape);
    }
    Ok(planes)
}

struct EdgeUse {
    edge: [u8; 2],
    faces: [usize; 2],
    forward: [bool; 2],
    count: usize,
}
fn edge_uses(faces: &[ConvexFace]) -> Result<Vec<EdgeUse>, ConvexError> {
    let mut edges: Vec<EdgeUse> = reserved(MAX_CONVEX_EDGES)?;
    for (face_index, face) in faces.iter().enumerate() {
        for index in 0..face.indices.len() {
            let a = face.indices[index];
            let b = face.indices[(index + 1) % face.indices.len()];
            let key = [a.min(b), a.max(b)];
            if let Some(edge) = edges.iter_mut().find(|edge| edge.edge == key) {
                if edge.count != 1 || edge.forward[0] == (a < b) {
                    return Err(ConvexError::InvalidShape);
                }
                edge.faces[1] = face_index;
                edge.forward[1] = a < b;
                edge.count = 2;
            } else {
                if edges.len() == MAX_CONVEX_EDGES {
                    return Err(ConvexError::Budget);
                }
                edges.push(EdgeUse {
                    edge: key,
                    faces: [face_index, 0],
                    forward: [a < b, false],
                    count: 1,
                });
            }
        }
    }
    if edges.iter().any(|edge| edge.count != 2) {
        return Err(ConvexError::InvalidShape);
    }
    edges.sort_unstable_by_key(|edge| edge.edge);
    Ok(edges)
}

fn connected_faces(edges: &[EdgeUse], faces: &[ConvexFace], vertex: Option<u8>) -> bool {
    let included = |face: usize| vertex.is_none_or(|v| faces[face].indices.contains(&v));
    let Some(first) = (0..faces.len()).find(|&f| included(f)) else {
        return false;
    };
    let mut reached = [false; MAX_CONVEX_FACES];
    let mut queue = [0; MAX_CONVEX_FACES];
    reached[first] = true;
    queue[0] = first;
    let (mut read, mut count) = (0, 1);
    while read < count {
        let face = queue[read];
        read += 1;
        for edge in edges {
            if vertex.is_some_and(|v| !edge.edge.contains(&v)) {
                continue;
            }
            if edge.faces.contains(&face) {
                for &adjacent in &edge.faces {
                    if !reached[adjacent] {
                        reached[adjacent] = true;
                        queue[count] = adjacent;
                        count += 1;
                    }
                }
            }
        }
    }
    (0..faces.len()).all(|face| !included(face) || reached[face])
}

fn validate_topology(
    vertices: &[[u16; 3]],
    faces: &[ConvexFace],
) -> Result<Vec<[u8; 2]>, ConvexError> {
    let edges = edge_uses(faces)?;
    if vertices.len() + faces.len() != edges.len() + 2 || !connected_faces(&edges, faces, None) {
        return Err(ConvexError::InvalidShape);
    }
    // Closed opposed edge incidences already give degree two to every incident face in a
    // vertex link. Connectivity additionally excludes multiple cycles pinched at one vertex.
    for vertex in 0..vertices.len() {
        if !connected_faces(
            &edges,
            faces,
            Some(u8::try_from(vertex).map_err(|_| ConvexError::Budget)?),
        ) {
            return Err(ConvexError::InvalidShape);
        }
    }
    Ok(edges.into_iter().map(|edge| edge.edge).collect())
}

fn physical_bounds(origin: IVec3, vertices: &[[u16; 3]]) -> Result<PhysicalBox, ConvexError> {
    let mut low = [u16::MAX; 3];
    let mut high = [0; 3];
    for vertex in vertices {
        for axis in 0..3 {
            low[axis] = low[axis].min(vertex[axis]);
            high[axis] = high[axis].max(vertex[axis]);
        }
    }
    let base = [origin.x, origin.y, origin.z].map(|v| i64::from(v) * 256_000_000);
    PhysicalBox::from_scaled(
        std::array::from_fn(|axis| base[axis] + i64::from(low[axis]) * 1_000_000),
        std::array::from_fn(|axis| base[axis] + i64::from(high[axis]) * 1_000_000),
    )
    .map_err(|_| ConvexError::Bounds)
}

fn fingerprint(
    origin: IVec3,
    vertices: &[[u16; 3]],
    faces: &[ConvexFace],
    material: Voxel,
) -> Result<u128, ConvexError> {
    let mut hash = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    let mut add = |bytes: &[u8]| {
        for byte in bytes {
            hash =
                (hash ^ u128::from(*byte)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
    };
    add(b"convex-material-shape-v1");
    for v in [origin.x, origin.y, origin.z] {
        add(&v.to_le_bytes());
    }
    add(&[material.material as u8, material.integrity]);
    add(&[u8::try_from(vertices.len()).map_err(|_| ConvexError::Arithmetic)?]);
    for vertex in vertices {
        for v in vertex {
            add(&v.to_le_bytes());
        }
    }
    add(&[u8::try_from(faces.len()).map_err(|_| ConvexError::Arithmetic)?]);
    for face in faces {
        add(&[u8::from(face.cut)]);
        add(&[u8::try_from(face.indices.len()).map_err(|_| ConvexError::Arithmetic)?]);
        add(&face.indices);
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Material;

    fn fixture() -> (Vec<[u16; 3]>, Vec<ConvexFaceInput>) {
        let vertices = vec![
            [0, 0, 0],
            [1_280, 256, 0],
            [1_280, 320, 0],
            [0, 64, 0],
            [0, 0, 768],
            [1_280, 256, 768],
            [1_280, 320, 768],
            [0, 64, 768],
        ];
        let faces = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 4, 7, 3],
            [1, 2, 6, 5],
            [0, 1, 5, 4],
            [3, 7, 6, 2],
        ]
        .into_iter()
        .enumerate()
        .map(|(index, indices)| ConvexFaceInput {
            indices: indices.to_vec(),
            cut: index != 5,
        })
        .collect();
        (vertices, faces)
    }
    fn build(
        vertices: &[[u16; 3]],
        faces: &[ConvexFaceInput],
    ) -> Result<ConvexFragment, ConvexError> {
        ConvexFragment::new(
            IVec3::new(-18, 1, 17),
            vertices,
            faces,
            Voxel::new(Material::Concrete),
        )
    }

    #[test]
    fn oblique_slab_has_canonical_faces_closed_edges_and_exact_scaled_bounds() {
        let (vertices, faces) = fixture();
        let fragment = build(&vertices, &faces).unwrap();
        assert_eq!(fragment.vertices().len(), 8);
        assert_eq!(fragment.faces().len(), 6);
        assert_eq!(fragment.edges().len(), 12);
        assert!(fragment.vertices().windows(2).all(|p| p[0] < p[1]));
        assert!(fragment.faces().windows(2).all(|p| p[0] < p[1]));
        assert_eq!(
            fragment.bounds().minimum_scaled(),
            [-18 * 256_000_000, 256_000_000, 17 * 256_000_000]
        );
        assert_eq!(
            fragment.bounds().maximum_scaled(),
            [-13 * 256_000_000, 576_000_000, 20 * 256_000_000]
        );
        assert_eq!(
            fragment.faces().iter().filter(|face| !face.cut()).count(),
            1
        );
        for (face, plane) in fragment.faces().iter().zip(fragment.planes()) {
            for &index in face.indices() {
                assert_eq!(
                    dot(
                        plane.normal(),
                        point(fragment.vertices()[usize::from(index)])
                    ),
                    plane.offset()
                );
            }
            assert!(
                fragment
                    .vertices()
                    .iter()
                    .all(|v| dot(plane.normal(), point(*v)) <= plane.offset())
            );
        }
        let mut reordered = vertices;
        reordered.reverse();
        let mut moved_faces = faces;
        for face in &mut moved_faces {
            for index in &mut face.indices {
                *index = 7 - *index;
            }
            face.indices.rotate_left(2);
        }
        moved_faces.reverse();
        assert_eq!(fragment, build(&reordered, &moved_faces).unwrap());
        moved_faces[0].cut = !moved_faces[0].cut;
        assert_ne!(
            fragment.fingerprint(),
            build(&reordered, &moved_faces).unwrap().fingerprint()
        );
    }

    #[test]
    fn missing_duplicate_reversed_star_and_nonplanar_faces_are_rejected() {
        let (vertices, faces) = fixture();
        for change in 0..7 {
            let mut faces = faces.clone();
            match change {
                0 => {
                    faces.pop();
                }
                1 => {
                    faces.push(faces[0].clone());
                }
                2 => faces[0].indices.reverse(),
                3 => faces[0].indices.swap(2, 3),
                4 => faces[0].indices[1] = faces[0].indices[0],
                5 => faces[0].indices[1] = 255,
                _ => faces[0].indices = vec![0, 3, 6, 1],
            }
            assert_eq!(
                build(&vertices, &faces),
                Err(ConvexError::InvalidShape),
                "case {change}"
            );
        }
        let mut dented = vertices;
        dented[2][1] = 200;
        assert_eq!(build(&dented, &faces), Err(ConvexError::InvalidShape));
    }

    #[test]
    fn air_empty_unused_duplicate_coplanar_and_out_of_bound_inputs_are_rejected() {
        let (vertices, faces) = fixture();
        assert_eq!(
            ConvexFragment::new(IVec3::default(), &vertices, &faces, Voxel::AIR),
            Err(ConvexError::InvalidShape)
        );
        assert_eq!(build(&[], &faces), Err(ConvexError::InvalidShape));
        let mut extra = vertices.clone();
        extra.push([100, 100, 100]);
        assert_eq!(build(&extra, &faces), Err(ConvexError::InvalidShape));
        let mut duplicate = vertices.clone();
        duplicate[7] = duplicate[6];
        assert_eq!(build(&duplicate, &faces), Err(ConvexError::InvalidShape));
        let coplanar: Vec<_> = vertices.iter().map(|&[x, _, z]| [x, 0, z]).collect();
        assert_eq!(build(&coplanar, &faces), Err(ConvexError::InvalidShape));
        let mut outside = vertices.clone();
        outside[0][0] = MAX_CONVEX_COORDINATE + 1;
        assert_eq!(build(&outside, &faces), Err(ConvexError::Bounds));
        for origin in [
            IVec3::new(i32::MIN, 0, 0),
            IVec3::new(MAX_CONVEX_ORIGIN + 1, 0, 0),
        ] {
            assert_eq!(
                ConvexFragment::new(origin, &vertices, &faces, Voxel::new(Material::Brick)),
                Err(ConvexError::Bounds)
            );
        }
        assert_eq!(
            build(&[[0; 3]; MAX_CONVEX_VERTICES + 1], &faces),
            Err(ConvexError::Budget)
        );
        assert_eq!(
            build(&vertices, &vec![faces[0].clone(); MAX_CONVEX_FACES + 1]),
            Err(ConvexError::Budget)
        );
        let mut oversized = faces;
        oversized[0].indices = vec![0; MAX_CONVEX_FACE_VERTICES + 1];
        assert_eq!(build(&vertices, &oversized), Err(ConvexError::Budget));
    }

    #[test]
    fn maximal_local_coordinates_and_both_origin_extremes_stay_exact() {
        let (mut vertices, faces) = fixture();
        for vertex in &mut vertices {
            vertex[0] = if vertex[0] == 0 {
                0
            } else {
                MAX_CONVEX_COORDINATE
            };
            vertex[2] = if vertex[2] == 0 {
                0
            } else {
                MAX_CONVEX_COORDINATE
            };
        }
        for extent in [-MAX_CONVEX_ORIGIN, MAX_CONVEX_ORIGIN] {
            let origin = IVec3::new(extent, extent, extent);
            let shape =
                ConvexFragment::new(origin, &vertices, &faces, Voxel::new(Material::Concrete))
                    .unwrap();
            assert_eq!(shape.origin(), origin);
            assert_eq!(shape.vertices().last().unwrap()[2], MAX_CONVEX_COORDINATE);
            assert_eq!(
                shape.bounds().maximum_scaled()[0] - shape.bounds().minimum_scaled()[0],
                2_048_000_000
            );
        }
    }
}
