# Unreal : état publié et provenance des captures

Publication documentaire du 10 septembre 2026, à partir des sessions natives du 8 septembre.
La première présentation GitHub était restée sur le prototype Rust du 6 septembre alors que
la production locale avait migré vers Unreal Engine 5.8.2. Ce point d'avancement corrige ce décalage.

## Le jalon actuel

Depuis le 7 septembre, Unreal est la direction de production. Le travail du 8 septembre a
abouti à deux jalons complémentaires : le rendu de l'usine Marble via Cesium for Unreal 2.29.1,
puis sa visite en première personne sur la carte distincte `MarbleWalk_v1`.

La couche visuelle utilise 4 320 000 Gaussian splats décodés côté CPU. Ce nombre n'est ni
un compteur de splats effectivement affichés par le GPU ni une mesure de performance.
Le collider est un maillage séparé ; rendu et collision partagent une transformation métrique recoupée.

Le checkpoint local `2026-09-08-marble-walk.md` et les reçus natifs établissent :

- compilation du module C++, création de la carte et relecture dans un processus neuf ;
- personnage et caméra actifs en Play In Editor Linux ;
- déplacement par événements clavier/souris, saut/réception et retour au départ avec R ;
- parcours de 6,36 m échantillonné dans la session de test ;
- captures du GameViewport, puis fin des sessions PIE contrôlée.

La qualification initiale du rendu s'effectue dans l'EditorViewport. Elle ne prouve pas à elle seule
la marche ; les deux captures du GameViewport et les essais de la session Marble Walk portent cette distinction.
Les événements clavier ont été testés séparément de l'API de session, qui n'injecte pas de déplacement.

## Images sélectionnées

| Capture du 8 septembre | Origine | Résolution | Ce qu'elle documente |
|---|---|---|---|
| [Vue générale](../screenshots/2026-09-08-unreal-marble-overview.png) | EditorViewport, Game View, `MarbleCesium_v1` | 1014 × 550 | Aspect de la scène 3D chargée dans Unreal |
| [Départ de la visite](../screenshots/2026-09-08-unreal-marble-walk.png) | GameViewport, PIE, `MarbleWalk_v1` | 1600 × 902 | Vue du personnage avec HUD après le test du retour au départ |
| [Vue rapprochée](../screenshots/2026-09-08-unreal-marble-walk-close.png) | GameViewport, PIE, `MarbleWalk_v1` | 1600 × 902 | Position après un parcours de 6,36 m et défauts visibles du détail proche |

Le [manifeste](../screenshots/2026-09-08-unreal-marble-manifest.json) conserve les identifiants
des reçus locaux, leurs empreintes, les résolutions et les SHA-256 des PNG. Les images publiées
sont identiques aux sorties natives : pas de recadrage, de retouche, de remplacement du HUD
ou de génération d'image supplémentaire. Elles montrent uniquement le viewport prévu.
Les reçus bruts et les assets générés restent locaux ; aucun lien privé de fournisseur n'est publié.

Les recettes C++/Python de ce travail étaient encore non commitées lors de la capture.
Le manifeste donne le HEAD local et les empreintes des sources propres au playtest, sans assimiler
ce HEAD seul au contenu exact compilé. Il ne constitue pas un manifeste de build complet.

## Limites de validation

La visite fonctionne sur le poste Linux de développement. Le détail proche est encore flou ou
déformé, le collider comporte des ouvertures et le parcours testé ne couvre pas toute la carte.
Le maintien prolongé des entrées humaines et le retour automatique après chute restent à confirmer.

La destruction par matériau, la construction, l'effondrement structurel et le multijoueur ne sont
pas portés et validés dans Unreal. Le raccord entre la géométrie physique modifiée et les splats
visibles reste à développer. Aucun package distribué, test sur machine propre, budget GPU ou
qualification Windows/macOS n'est annoncé.

Lors de cette publication, le doctor local confirme UE 5.8.2, la présence de l'éditeur et du projet,
ainsi que les gardes Trace et VibeUE. Il ne relance pas l'éditeur ni les essais de gameplay du
8 septembre. Les PNG, leurs reçus et les sources ciblées ont été relus et comparés ; cette
intervention documentaire ne constitue pas une nouvelle campagne de tests du moteur.

## Publication et attributions

Le monde représenté a été généré via l'API World Labs / Marble à partir du concept industriel
original choisi pour le projet, puis rendu nativement dans Unreal avec Cesium for Unreal.
Les captures ne sont pas des photographies ni des images promotionnelles directement générées.

La section 3.3(d) des [conditions officielles World Labs](https://docs.worldlabs.ai/terms-of-service),
consultées le 10 septembre 2026, prévoit des droits de reproduction, adaptation et distribution
des résultats obtenus via API, sous leurs conditions. La sélection publique est limitée aux trois
captures demandées ; l'origine World Labs / Marble est indiquée et les images ne sont pas altérées.
Cette vérification ne confère aucune licence globale aux assets du dépôt.

Cette publication contient la présentation, les captures et leur provenance. La consolidation
des sources Unreal reste distincte : le lanceur, les plugins et les assets locaux nécessaires au
playtest ne sont pas distribués dans ce commit. Le code et la documentation du prototype Rust
restent accessibles comme références historiques.
