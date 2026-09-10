# Reprendre le projet Destructible FPS

**Direction actuelle : Unreal Engine 5.8.2.** Lire le [point public Unreal du 10 septembre](docs/checkpoints/2026-09-10-unreal-presentation.md) et les [captures natives du 8 septembre](README.md#captures-unreal). La présentation et les captures sont publiées ; les sources de migration restent en cours de consolidation locale.

Le contenu ci-dessous conserve le point de reprise historique Rust du 6 septembre. Ses priorités de moteur et de rendu ne remplacent pas la direction Unreal actuelle.

Point de reprise du 6 septembre 2026. Ce fichier est l'entrée humaine et agent du projet ; il ne
dépend ni du contexte d'une conversation, ni d'une mémoire privée, ni d'un dossier `/tmp`.
Les sources, tests et reçus versionnés priment sur un résumé. Lire aussi [AGENTS.md](AGENTS.md).

Sources distantes : [GitHub public](https://github.com/AdrienAvalon/destructible-fps) et
[GitLab privé](https://gitlab.avalon-network.com/avalon/destructible-fps). Les deux doivent pointer
sur le même commit `main` après une sauvegarde. Aucun miroir automatique n'est installé : vérifier
les deux accusés de push et les refs distantes, sans forcer ni partager les identifiants dans Git.

## Objectif, sans réduction d'ambition

Créer un FPS natif **réellement photoréaliste**, magnifique, multijoueur et multi-OS, avec construction,
destruction persistante et physique crédible. La référence industrielle ci-dessous est une direction
artistique générée, **pas une capture du jeu**. L'ambition est de l'atteindre, voire de la dépasser.

![Objectif artistique généré — ce rendu n'est pas atteint par le moteur](docs/concepts/photoreal-industrial-target-v1.png)

- Les matériaux réagissent différemment : du bois cède sous des tirs répétés, une charge peut ouvrir
  un mur, la réponse dépend du matériau, de l'épaisseur et des dégâts accumulés. Ces exemples sont
  des exigences de jeu, pas des valeurs réelles d'armement ni une calibration déjà validée.
- Un bâtiment privé de supports doit redistribuer ses charges, se fissurer, se séparer et s'effondrer
  de façon crédible. Ne pas confondre un objet détaché par connectivité avec cet objectif complet.
- Le serveur décide des mutations persistantes ; les joueurs doivent observer un état cohérent.
  Qualité visuelle, latence, sécurité, stabilité temporelle et performances se valident séparément.
- Le monde visible ne doit pas avoir une coque décorative indépendante de ses volumes physiques,
  de faux trous, de lambeaux flottants ou de couvertures indestructibles ajoutées pour embellir.
- À terme : génération de cartes depuis recettes, envies ou photos ; modes, scénarios, événements
  et PNJ réutilisables ; outils appelables par un agent pour créer, jouer, inspecter et rejouer ;
  directeur adaptatif optionnel compatible avec des modèles locaux ou des API explicitement activées.
  Ces extensions restent prévues, sans détourner la priorité immédiate du rendu natif.

Contrat détaillé : [game-contract](docs/game-contract.md). Étapes, budgets et critères de sortie :
[production-roadmap](docs/production-roadmap.md). Le budget visé n'est pas une performance acquise.

## État réel au checkpoint

**Le rendu ne satisfait pas l'objectif photoréaliste. L'utilisateur ne l'a pas accepté.**
Ce checkpoint sauvegarde sérieusement une base technique, pas une version finale, ni une validation
artistique. Voir [les trois images natives et les preuves](docs/checkpoints/2026-09-06.md).

| Chemin | Ce qui existe | Limite à ne pas oublier |
| --- | --- | --- |
| `playable-demo --world industrial` | FPS local jouable, tirs/rechargement, explosion, construction, débris autoritaires | Monde principalement à cellules d'un mètre ; pas la dernière scène convexe |
| `multiplayer-demo` et serveurs dédiés | Réplication, snapshots/réparation, prédiction ; chemin QUIC/OIDC testé | Serveurs limités au loopback ; pas de partie Internet/LAN prête à publier |
| `fine-geometry-demo --world industrial` | Inspection native fine, quatre états préparés, fragments obliques, requêtes exactes | Pas de combat fin, effondrement dynamique fin ou réplication des fragments convexes |
| Laboratoire structurel | Solveur et adaptateur de rupture bornés, cas analytiques et intégration expérimentale | Pas de calibration complète ni effondrement photoréaliste généralisé |
| Production/IA | Contrats écrits, cookers et outils de mesure utilisables | Pas d'éditeur général, importeur GLB du moteur, serveur MCP du jeu ou directeur IA livré |

La plateforme effectivement testée ici est Linux x86_64/Vulkan. Windows, macOS, Metal/DX12,
distribution et installations sur machines propres restent des critères de sortie ouverts.

## Tester ou reconstruire

Depuis la racine du dépôt, avec Rust/Cargo disponibles dans le `PATH` :

```bash
cargo build --locked --release --bin playable-demo --bin fine-geometry-demo
./target/release/playable-demo --world industrial
# Fermer le FPS avant de comparer la vue d'inspection :
./target/release/fine-geometry-demo --world industrial --view approach
```

FPS : cliquer pour capturer la souris ; ZQSD/WASD, Espace, Maj ; clic gauche pour tirer, R pour
recharger, clic droit pour l'explosion expérimentale, clic molette pour construire du bois.
Échap libère la souris, puis quitte. Inspection : haut/bas pour tourner, W/S pour zoomer,
gauche/droite pour sélectionner l'état, P pour sonder le matériau, Échap pour quitter.
Les vues nommées sont `approach`, `wide`, `fracture`.

`Cargo.lock` et les versions directes exactes de `Cargo.toml` sont versionnés. `rust-version = 1.97`
est un minimum, **pas** un toolchain exact imposé ; la preuve locale utilise Rust/Cargo 1.97.1.
Les packs matériaux/HDR sont dans Git ; un build ordinaire ne télécharge pas de textures.
Les recettes de recuisson ont leurs propres versions strictes : ne pas les assouplir pour passer
un contrôle sur une autre machine. [Matériaux](assets/materials/README.md),
[environnement](assets/environment/README.md).

## Première tâche à la reprise

Ne pas repartir sur des micro-ajustements isolés de shader et ne pas déclarer la cible atteinte
parce que les tests sont verts. Construire **une petite zone complète convaincante à hauteur de
joueur**, avec les mêmes vues et budgets, avant d'élargir la carte ou les outils génériques.

1. Comparer les images natives du checkpoint au concept : proportions/silhouettes, contacts des
   débris, densité, matière, terrain, éclairage intérieur/extérieur. Noter les écarts visibles.
2. Remplacer l'arrangement clairsemé de dalles sur supports congruents par des gravats crédibles
   à plusieurs échelles, contacts irréguliers et épaisseurs de fracture cohérentes.
3. Livrer le pont de production nécessaire à du vrai contenu natif : géométrie, échelle métrique,
   provenance, plafonds et correspondance physique validés. Le smoke Blender n'est pas ce pont.
4. Composer les transitions de sol, détails de bâtiment, végétation et plans de fond ; améliorer
   profondeur lumineuse et matières sans cacher les problèmes géométriques sous le post-traitement.
5. Vérifier trois vues fixes **et le mouvement**, avant/après ; mesurer CPU et GPU séparément sans
   compilation ou capture instrumentée simultanée. Conserver les ratés et expliquer les rejets.
6. Une fois ce jalon visuel accepté, promouvoir la géométrie fine vers armes, structure, corps et
   réplication avec tests dédiés. Ne pas brancher le composite d'inspection comme s'il était déjà
   implémenté dans les contrats `StaticGeometry`, codecs ou autorité.

Un nouvel outil ne mérite d'être ajouté que s'il résout un obstacle démontré. Conserver les bornes,
les anciennes scènes de régression et un rollback. Pas de réécriture globale improvisée du moteur.

## Carte de lecture et historique

| Sujet | Source de reprise |
| --- | --- |
| Objectif et validation artistique | [visual-direction](docs/visual-direction.md), [industrial-visuals](docs/industrial-visuals.md) |
| Architecture et performances | [architecture](docs/architecture.md), [performance](docs/performance.md) |
| Dernière géométrie exacte | [convex-inspection](docs/convex-inspection.md), `src/convex/`, `tests/convex_scene.rs` |
| Préparation, upload, sonde du viewer | `src/bin/fine_geometry_demo/{fixed,stream,probe}.rs`, `src/mesh/fine/fixture.rs` |
| Représentation fine et requêtes | [refined-volumes](docs/refined-volumes.md), [world-geometry](docs/world-geometry.md), [static-collision](docs/static-collision.md) |
| Masse, tirs et structure | [mass-properties](docs/mass-properties.md), [ballistics](docs/ballistics.md), [fine-ballistics](docs/fine-ballistics.md), [structural-runtime](docs/structural-runtime.md) |
| Réseau et sécurité | [secure-server](docs/secure-server.md), [remote-exposure-gate](docs/remote-exposure-gate.md), [lan-deployment-policy](docs/lan-deployment-policy.md) |
| Cartes, scénarios, outils et IA futurs | [map-generation](docs/map-generation.md), [gameplay-authoring](docs/gameplay-authoring.md), [agent-tooling](docs/agent-tooling.md), [adaptive-director](docs/adaptive-director.md) |
| Références techniques et assets | [references](docs/references.md), manifestes dans `assets/` |

L'historique Git complet garde les étapes, pas seulement ce résumé : `git log --reverse --oneline`.
Repères importants avant ce checkpoint :

| Commit | Étape |
| --- | --- |
| `fbfd50b`, `397ca64` | Noyau déterministe puis première démo Vulkan |
| `a4c6300`, `ae79f31`, `4488131`, `5eec27a` | Serveur, QUIC, client multi puis chemin graphique authentifié |
| `cf8c18a` | Scans PBR et correction des jonctions de brèche hybrides |
| `aefb6f6`, `08aca03` | Équilibre structurel puis runtime de rupture expérimental |
| `6707104` | Outils de capture/profilage/authoring confinés |
| `2e549ae`, `e4ef8fe`, `6e7df1d` | HDR/IBL, visibilité du ciel, présentation HDR/MSAA |
| `b1e50ee`, `68ab538` | Site industriel jouable et fusil directionnel |
| `8fd7231` à `422c32e` | Volumes fins, requêtes/masse et inspection native intégrée au terrain |
| `fb77399`, `ad0e2e0` | Contrats outils/scénarios/IA et priorité visuelle |
| `e6cf620` | Haze coûteuse rejetée, retour au runtime précédent |
| `cedb136` | Bâtiment ruiné supporté ; défaut d'interpolation des coupes corrigé par test raster |
| `575aa25` | Fragments convexes natifs, requêtes exactes et publication bornée dans l'inspecteur |

Le présent checkpoint ajoute les fragments convexes obliques et leur intégration **d'inspection**.
Leurs limites et les corrections retenues après Claude sont dans `convex-inspection.md`.

## Outils réellement utilisés et portabilité

| Outil | Ce qui est conservé et son rôle réel |
| --- | --- |
| Rust/Cargo, Clippy, tests, benchmarks | Sources et matrice [validate_checkpoint.sh](tools/validate_checkpoint.sh) ; simulation, parsers, réseau, géométrie et distributions |
| Python/ImageMagick/zlib | Cookers matériaux et HDR, manifestes hashés et tests `tools/test_*.py` |
| RenderDoc | `tools/tooling_smoke.py`, `tools/renderdoc_smoke.py` : vrai frame Vulkan/replay, pas un benchmark non instrumenté |
| Blender | `tools/blender_smoke.py` : rendu et aller-retour GLB contrôlé ; pas encore un importeur dans le jeu |
| Tracy | Client de test C++, wrapper viewer et configuration dans `tools/` ; pas d'instrumentation Rust livrée |
| perf, CMake, Ninja | Mesures CPU et compilation d'outils ; recettes et versions dans [tooling](docs/tooling.md) |
| Bubblewrap | Confinement réseau/PID et sorties bornées des outils ; ce n'est pas une barrière contre tout fichier hôte lisible |
| FFmpeg, ffprobe, xdotool | Spot-check vidéo d'une fenêtre possédée ; reçu local, pas encore un harness portable versionné |
| Graphify | Index AST dérivé local ; wrapper du dépôt d'infra adjacent, pas une dépendance du jeu |
| Codex, sous-agents, Claude | Implémentation, vérifications et avis bornés ; pas de secrets/transcripts privés dans Git, pas de modèle intégré au runtime |

Les scripts de production sont versionnés, pas les installations des logiciels, caches, outils
d'authentification ou profils personnels. [toolchain-observed.json](tools/toolchain-observed.json)
est un reçu de versions observées, pas un installeur. Le jeu ne dépend pas de l'infra adjacente
pour compiler. Aucun changement des droits de l'agent, de ses réglages de raisonnement ou des
services de l'infrastructure n'est requis par ce checkpoint.

Sur le poste original uniquement, si le dépôt adjacent et son wrapper existent :

```bash
../infra_avalon/scripts/graphify.sh --repo "$PWD" status
../infra_avalon/scripts/graphify.sh --repo "$PWD" update
../infra_avalon/scripts/graphify.sh --repo "$PWD" doctor
```

Lire la skill locale avant utilisation ; `graphify-out/` reste ignoré, sans hooks ni backend LLM
implicite. Confirmer le graphe dans les sources. Pour Claude, relire la skill `claude-pair` du
dépôt adjacent ; fournir seulement un contexte expurgé, pas des accès au compte ou à l'infra.
Ces helpers ne sont pas copiés depuis l'infra et ne doivent pas devenir une condition de build.

## Valider puis sauvegarder

```bash
bash tools/validate_checkpoint.sh
# Uniquement lorsque la session graphique est libre et un GPU Vulkan réel disponible :
bash tools/validate_checkpoint.sh --gpu
git diff --check
```

Le mode CPU ne valide pas le rendu ; les tests GPU ignorés ne sont jamais comptés comme passés.
Les exécutions GPU exigent des preuves effectives et peuvent utiliser une fenêtre : ne pas lancer
pendant une partie utilisateur. Le script ne modifie pas les sources, ne crée pas de compte et ne
pousse rien ; ses logs sont locaux/ignorés. Les timings avec compilation/capture ne sont pas une
preuve d'optimisation. Refaire des mesures isolées si le moteur, pilote ou matériel change.

À chaque reprise : lire ce fichier et le dernier reçu, inspecter branche/diff/distant, préserver les
changements tiers, fixer un petit lot avec critère de sortie, puis mettre à jour documents, preuves
et tests ensemble. Faire des commits nommés et cohérents ; ne pas réécrire l'historique partagé.
Une release binaire nécessite encore recette de paquet, mentions de dépendances, tests depuis une
machine propre et scope de plateformes exact. Ne pas présenter `version = 0.1.0` comme une release
publiée. Le code reste `UNLICENSED` et `publish = false` ; les assets CC0 gardent leurs propres droits.

Les trois images natives choisies sont une exception documentaire explicitement demandée aux captures
de travail normalement ignorées. Ne pas versionner `target/`, `.rdc`, traces, conversations, dumps
du bureau, jetons ou clés. Un chemin `/tmp` peut disparaître ; un chemin `target/` n'est pas une
sauvegarde hors machine. Le dépôt conserve objectif, images sélectionnées, recettes et reçus ;
il ne prétend pas archiver chaque sortie temporaire ou l'intégralité d'une conversation.
