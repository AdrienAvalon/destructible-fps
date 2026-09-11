**Français** · [English](README.en.md)

<div align="center">

# Destructible FPS

**Explorer une usine en ruine. Construire un monde qui garde la trace des impacts.**

Un FPS en développement sous **Unreal Engine 5.8.2**, consacré à la destruction persistante.
Le jalon actuel : une visite en première personne dans un environnement industriel, sur le poste Linux de développement.

[Voir la vidéo](#vidéo) · [Captures](#captures-unreal) · [Avancement](#état-du-projet) · [Explorer le dépôt](#explorer-le-dépôt) · [Suite](#prochaines-étapes)

[![Direction de production : Unreal Engine 5.8.2](https://img.shields.io/badge/moteur-Unreal%20Engine%205.8.2-313131?style=flat-square&logo=unrealengine&logoColor=white)](#état-du-projet)
[![Jalon : visite FPS locale](https://img.shields.io/badge/jalon-visite%20FPS%20locale-8b7cf6?style=flat-square)](#vidéo)
[![Code publié : prototype Rust](https://img.shields.io/badge/code%20publi%C3%A9-prototype%20Rust-2496ed?style=flat-square&logo=rust&logoColor=white)](docs/rust-prototype.md)
[![Licence du code : UNLICENSED](https://img.shields.io/badge/licence%20du%20code-UNLICENSED-lightgrey?style=flat-square)](#droits-et-attributions)

<img src="docs/screenshots/2026-09-08-unreal-marble-overview.png" alt="Capture native dans Unreal Engine du 8 septembre 2026 : usine en ruine, gravats, sol humide et falaises." width="880">

*Vue native de l'éditeur Unreal, 8 septembre 2026. Environnement 3D généré avec World Labs / Marble, rendu avec Cesium for Unreal.*

</div>

## Vidéo

<div align="center">

<a href="docs/videos/2026-09-10-marble-walk.mp4"><img src="docs/videos/2026-09-10-marble-walk-preview.gif" alt="Aperçu animé natif de la marche dans Marble Walk. Ouvrir la vidéo complète." width="640"></a>

[**Voir la visite complète · 32 s · 720p · MP4**](docs/videos/2026-09-10-marble-walk.mp4)

</div>

Enregistrée le **10 septembre 2026** dans le GameViewport Unreal sous Linux, cette vidéo
silencieuse montre un panoramique, une marche d'environ **6,3 m**, un saut avec réception
et le retour au départ. L'aperçu animé ci-dessus reprend quatre secondes de cette même session.

Le détail proche reste imparfait : il s'agit d'une visite de prototype, sans destruction ni
multijoueur. Les [preuves de cette session](docs/checkpoints/2026-09-10-unreal-video.md)
précisent le parcours, la capture et les limites de validation.

## État du projet

**Unreal Engine 5.8.2 est la direction de production depuis le 7 septembre 2026.**
Le prototype Rust reste une référence pour la simulation et le réseau. Les images ci-dessus
et ci-dessous montrent les avancées Unreal du 8 septembre, désormais présentées dans ce dépôt.

Le dernier jalon, **Marble Walk**, permet de parcourir une scène industrielle en première
personne dans l'éditeur Linux. Le rendu utilise un ensemble de points volumétriques
(*Gaussian splats*) issu du monde Marble ; un maillage de collision séparé permet la marche.

<details>
<summary><strong>Consulter le détail des essais et du travail restant</strong></summary>

| Domaine | Ce qui fonctionne dans Unreal | Ce qui reste à valider ou développer |
|---|---|---|
| **Environnement industriel** | Scène importée, sauvegardée, relue et rendue nativement avec Cesium | Détail proche, stabilité visuelle en mouvement et budget GPU |
| **Visite en première personne** | Personnage et caméra, entrées clavier/souris, saut/réception, retour au départ ; parcours testé de 6,36 m | Couverture complète du terrain, maintien clavier prolongé et récupération automatique après chute |
| **Collision** | Maillage source distinct, contacts au sol et positions du parcours recoupés | Maillage ouvert ; tous les obstacles et limites de la carte ne sont pas qualifiés |
| **Destruction et construction** | Objectifs et références techniques du prototype Rust conservés | Combat, dégâts par matériau, effondrement et modification cohérente du décor Unreal |
| **Multijoueur et distribution** | Exigences d'autorité serveur et de persistance conservées | Réplication Unreal, package jouable, autres machines et autres systèmes |

</details>

Le jalon actuel est une **visite technique locale**, avec un rendu encore imparfait de près.
Les fonctionnalités de destruction du prototype Rust ne sont pas présentées comme déjà portées dans Unreal.
Le [point d'avancement et les preuves de capture](docs/checkpoints/2026-09-10-unreal-presentation.md)
précisent les essais réalisés et les limites de cette publication.

## Captures Unreal

Ces images proviennent des sessions natives du **8 septembre 2026**. Les deux vues suivantes
ont été prises dans le **GameViewport en Play In Editor**, avec le personnage et son HUD.
Les PNG sont publiées à l'identique ; aucune retouche ou génération d'image n'a été ajoutée.

<table>
  <tr>
    <td width="50%" align="center"><strong>Au départ de la visite</strong></td>
    <td width="50%" align="center"><strong>Au pied du bâtiment</strong></td>
  </tr>
  <tr>
    <td><a href="docs/screenshots/2026-09-08-unreal-marble-walk.png"><img src="docs/screenshots/2026-09-08-unreal-marble-walk.png" alt="Capture native du GameViewport Unreal au départ de Marble Walk, avec viseur et HUD du prototype." width="100%"></a></td>
    <td><a href="docs/screenshots/2026-09-08-unreal-marble-walk-close.png"><img src="docs/screenshots/2026-09-08-unreal-marble-walk-close.png" alt="Capture native après un parcours de 6,36 mètres : façade et gravats vus de près, avec des détails encore flous et déformés." width="100%"></a></td>
  </tr>
  <tr>
    <td>Après le retour au départ avec R.</td>
    <td>Le détail proche reste à améliorer.</td>
  </tr>
</table>

Cliquer sur une image pour l'ouvrir à sa taille d'origine.

Les environnements 3D ont été générés avec **World Labs / Marble**, puis intégrés et capturés
dans Unreal. Le [manifeste des images](docs/screenshots/2026-09-08-unreal-marble-manifest.json)
conserve leur origine, leur résolution et leur empreinte SHA-256.

## Explorer le dépôt

Les commandes concernent les utilisateurs autorisés ; consulter les [droits de réutilisation](RIGHTS.md).

| Vous cherchez… | Point d'entrée |
|---|---|
| **Les avancées Unreal** | [Jalon et provenance des captures](docs/checkpoints/2026-09-10-unreal-presentation.md) · [Preuves de la vidéo](docs/checkpoints/2026-09-10-unreal-video.md) |
| **Du code à exécuter aujourd'hui** | [Guide du prototype Rust](docs/rust-prototype.md) : démo FPS, multijoueur local et inspection de géométrie sous Linux/Vulkan |
| **Les choix techniques** | [Architecture de la simulation et du réseau du prototype](docs/architecture.md) |

**Le contenu Unreal publié se limite à la présentation, aux captures et à leur provenance.**
Les sources de migration, le lanceur et les assets du playtest local ne sont pas encore inclus.
Un clone de ce dépôt permet d'explorer le prototype Rust ; aucun package Unreal jouable n'est proposé.

Le prototype Rust conserve les références de simulation, de destruction et d'autorité serveur.
Ses commandes et anciennes captures sont regroupées dans son guide ; ses capacités ne constituent
pas une preuve de leur portage dans Unreal.

<details>
<summary><strong>Relancer la visite Unreal sur le poste de développement déjà préparé</strong></summary>

La carte locale s'appelle `MarbleWalk_v1`. Sur le poste où le moteur, les plugins et les
assets ont déjà été préparés, la commande existante ouvre directement ce playtest :

```bash
python3 tools/unreal.py editor --playtest marble-walk --x11
```

Cliquer **Jouer**, puis dans la vue du jeu pour lui donner le focus.

| Commande configurée | Action |
|---|---|
| `ZQSD` / `WASD` / flèches | Marcher |
| Souris | Regarder |
| `Espace` | Sauter |
| `R` | Revenir au départ |
| `Échap` / `Maj+F1` | Arrêter PIE / libérer le curseur |

Les essais ont exercé W, Espace, R et la souris ; chaque variante de clavier n'a pas été testée.

Ces instructions concernent le checkout local de développement ; le fichier `tools/unreal.py`
n'est pas fourni dans cette publication GitHub.

</details>

## Prochaines étapes

Les étapes suivantes décrivent la direction du travail ; elles ne sont pas encore livrées.

1. **Fiabiliser la visite** : vérifier un trajet plus long, les obstacles, les chutes et les contrôles.
2. **Mesurer la scène** : temps CPU/GPU, mémoire et stabilité du rendu au cours du déplacement.
3. **Construire une petite zone destructible** : matériaux distincts, collision et décor mis à jour ensemble.
4. **Relier simulation et multijoueur Unreal** : autorité serveur, persistance et convergence des clients.
5. **Préparer une distribution testable** : package, installation et validation sur une machine propre.

## Droits et attributions

Le code original publié reste **UNLICENSED** : ses droits sont [réservés](RIGHTS.md).
Sa réutilisation et son exploitation commerciale nécessitent un accord écrit préalable,
avec une rémunération convenue pour l'exploitation commerciale du contenu propriétaire.
Les dépendances et assets tiers conservent leurs propres licences, notamment les matériaux et
l'environnement Poly Haven sous CC0. Le [guide Rust](docs/rust-prototype.md#droits-et-attributions)
renvoie à leurs sources et attributions.

Les captures Unreal créditent World Labs / Marble pour la génération du monde et Cesium for Unreal
pour son rendu dans le moteur Epic. La publication de ces vues ne redistribue ni le moteur Epic,
ni les plugins installés, ni les fichiers source du monde généré.
La [note de provenance](docs/checkpoints/2026-09-10-unreal-presentation.md#publication-et-attributions)
documente les conditions vérifiées pour ces captures.
