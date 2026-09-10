# Marble Walk : visite native en vidéo

Une [vidéo de 32 secondes](../videos/2026-09-10-marble-walk.mp4) complète les captures du
8 septembre. Elle a été enregistrée le **10 septembre 2026** dans le GameViewport de
`MarbleWalk_v1`, en Play In Editor sous Unreal Engine 5.8.2, Linux/Vulkan.

## Ce que montre la séquence

La caméra regarde de part et d'autre du bâtiment, puis le personnage avance par quatre
impulsions clavier sur environ **6,3 m**. Un saut, sa réception et un retour au départ avec
R complètent la visite. Le HUD existant reste visible. La séquence est silencieuse.

Les états natifs relus pendant l'enregistrement confirment le personnage possédé, la caméra
active, le chargement des 4 320 000 splats côté CPU et les contacts au sol sur ce trajet.
Le saut comporte un état hors sol suivi d'une réception ; R incrémente le compteur de retour
et rétablit la position de départ. Les entrées proviennent d'événements XTEST bornés envoyés
uniquement à la fenêtre du jeu. Cette séquence ne remplace pas un test de tous les claviers,
de maintiens prolongés ou de perte de focus.

## Capture et fichier publié

L'éditeur a été lancé par le runner existant, après vérification de ses gardes Trace et VibeUE.
Un affichage X11 dédié et authentifié ne contenait que les fenêtres de cette session Unreal.
FFmpeg a capturé l'identifiant de la fenêtre PIE appartenant au processus lancé pour ce travail.
Le bureau personnel, les notifications, le pointeur et les autres applications ne sont pas enregistrés.

La fenêtre de 1606 × 940 pixels a été recadrée à son viewport : 1600 × 900 depuis `(3, 35)`.
Les 32 premières secondes de la prise continue de 38 secondes sont conservées, redimensionnées
en **1280 × 720**, puis encodées en **MP4/H.264, yuv420p, 30 images/s**, avec démarrage rapide.
Aucune coupe intermédiaire, accélération, interpolation, retouche de scène ou image générée
n'a été ajoutée. Le fichier brut reste local ; seule cette sélection compacte est publiée.

L'[aperçu animé](../videos/2026-09-10-marble-walk-preview.gif) reprend les secondes 12 à 16
du même MP4 : 480 × 270, huit images/s, palette de 64 couleurs. C'est un aperçu en boucle ;
le MP4 conserve la séquence entière et une meilleure qualité.
Le [manifeste](../videos/2026-09-10-marble-walk.manifest.json) fournit les empreintes,
le cadrage, les dimensions, les actions observées et les limites.

## Vérifications et préservation

- Décodage intégral du MP4 sans erreur : **960 images, durée 32 secondes**, une seule piste vidéo.
- Lecture et déplacement dans la vidéo vérifiés dans Chrome, en contexte neuf avec sandbox.
- Images réparties dans la séquence inspectées ; HUD et viewport restent visibles, sans bureau.
- Session PIE terminée avec reçu persistant, sans erreur et sans package sale.
- Éditeur, serveur X11 et processus Zen propres à cette capture arrêtés ; ports MCP, Trace et Zen fermés.
- État Git local et **1 598 fichiers** de sources, configurations et assets protégés inchangés.

Les avertissements connus du plugin Cesium n'ont pas été masqués. Cette vidéo documente une visite
locale du prototype : elle ne prouve ni destruction, ni multijoueur, ni qualité finale des surfaces
proches, ni couverture exhaustive des collisions. Les 30 images/s du fichier sont une cadence
d'enregistrement, pas une mesure qualifiée des performances du moteur.

## Origine du monde

Le monde 3D a été généré avec **World Labs / Marble**, puis rendu nativement dans Unreal
avec **Cesium for Unreal**. Les [attributions et droits vérifiés pour les captures](2026-09-10-unreal-presentation.md#publication-et-attributions)
s'appliquent à cette même scène. Aucun asset source, plugin ou moteur tiers n'est redistribué.
