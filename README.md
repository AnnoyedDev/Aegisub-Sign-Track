# AegiSignTrack

**English: [README.en.md](README.en.md)**

Outil de tracking, de compositing et de typesetting pour Aegisub : il suit un élément de la vidéo,
puis fabrique les lignes ASS qui vont avec. Tout le travail sur un sign peut se faire au même endroit.

- **Tracking** : suivi d'un point, de plusieurs points ou d'une zone, en déplacement, échelle, rotation,
  perspective ou déformation : il peut prendre en compte les fondus (opacités), les objets cachés ou sortis de l'image.
- **Cache** : masquage de la zone suivie par un aplat, des dégradés ou une image vectorisée (l'image vectorisée peut rendre le script **__EXTRÊMEMENT__** lourd).
- **Sign / typeset** : texte posé sur la vidéo, avec police, couleurs, contour, ombre et flou, ou lignes
  déjà écrites dans Aegisub qui reçoivent le mouvement.
- **Export** : copie dans le presse-papiers ou envoi direct à Aegisub par le plugin.
- **Export Basique** : Vous pouvez, si vous le souhaitez exporter simplement le tracking.

Deux morceaux qui travaillent ensemble :

- `aegisigntrack` : le logiciel (Rust, Linux et Windows).
- `aegisub/aegisigntrack.lua` : le plugin Aegisub qui lui envoie la sélection et récupère le résultat.

Développé par AnnoyedDev.

## Installation

Il faut `ffmpeg` et `ffprobe` : dans le PATH, ou posés à côté de l'exécutable, ou dans un dossier indiqué
avec *Dossier de ffmpeg...* en bas des réglages.

Le plus simple : prendre la dernière release, qui contient l'exécutable (Linux ou Windows) et le plugin.
Une release est publiée à chaque commit. Son numéro, date et commit, est aussi affiché dans le titre de
la fenêtre.

Pour compiler soi-même :

Téléchargez la dernière [release](https://github.com/AnnoyedDev/Aegisub-Sign-Track/releases) ou compilez vous même avec :

```sh
cargo build --release
```

L'exécutable est `target/release/aegisigntrack`.

Pour Windows :

- sur Windows même : installer Rust (https://rustup.rs), puis `cargo build --release` dans ce dossier,
  l'exécutable est `target\release\aegisigntrack.exe`.
- depuis Linux : `rustup target add x86_64-pc-windows-gnu`, installer `mingw-w64-gcc`, puis
  `cargo build --release --target x86_64-pc-windows-gnu`, l'exécutable est dans
  `target/x86_64-pc-windows-gnu/release/aegisigntrack.exe`.

Copier `aegisub/aegisigntrack.lua` dans le dossier `automation/autoload` d'Aegisub
(`~/.aegisub/automation/autoload/` sous Linux, `%APPDATA%\Aegisub\automation\autoload\` sous Windows).
Si `aegisigntrack(.exe)` n'est pas dans le PATH, indiquer son emplacement dans Aegisub avec
*Automatisme > AegiSignTrack > Chemin du tracker*.

## Utilisation

**Depuis Aegisub** : sélectionner la ou les lignes, puis *Automatisme > AegiSignTrack > Tracker la
sélection*. Le tracker s'ouvre sur la portion de vidéo couverte par les lignes. Une fois le
tracking fait, *Envoyer à Aegisub* insère les lignes dans le script (annulable avec Ctrl+Z).
ou vous pouvez simplement copier avec *Copier pour Aegisub* et coller là où vous le souhaitez.

**À la main** : lancer `aegisigntrack(.exe)`, ouvrir une vidéo (ou la glisser dans la fenêtre), choisir la
plage à charger. *Copier pour Aegisub* met les lignes dans le presse-papiers puis les coller dans la
grille d'Aegisub.

Dans AegiSignTrack, le panneau de droite a trois modes : le clic sur la vidéo agit sur le mode affiché.

1. **Tracking** : choisir **Points** (1 point ou plus, chacun suivi individuellement) ou **Zone**
   (polygone dont toute la surface est suivie), cliquer sur la vidéo pour dessiner la sélection, cocher
   les mouvements à suivre et régler la recherche. Puis *Tracker ▶*, *◀ Tracker* ou *◀ Tout ▶* depuis la
   frame affichée, la plage se règle avec `[` et `]`. Si le suivi dérape : se placer sur la frame
   fautive, glisser la sélection au bon endroit, relancer de là.
2. **Cache** : cocher *Exporter le cache*. Par défaut il reprend la sélection, cliquer sur la vidéo pour
   en dessiner un autre où l'on veut, il suivra le tracking depuis cet endroit. Couleur (avec pipette sur
   la vidéo) et flou. *Dégradés* : chaque dégradé fond sa couleur sur celle du cache, dans le sens et
   avec la force choisis, plusieurs dégradés se superposent. *Image* : une image (PNG ou autre) prend
   la place du cache, étirée sur le rectangle qui l'entoure.
3. **Texte** : cocher *Exporter le texte*, cliquer sur la vidéo pour le placer. *Personnaliser
   l'apparence* remplace le style de la ligne : police (liste des polices installées), taille, gras,
   italique, couleur, contour, ombre, flou.

Le cache et le texte sont décochés au départ. Si aucun des deux n'est coché, *Copier* et *Envoyer*
demandent confirmation puis ne produisent que le tracking : chaque ligne sélectionnée garde son texte
et sa position (`\pos`, sinon le centre de la zone) et reçoit position, taille et rotation frame par
frame. Hors Aegisub, ce sont des lignes vides portant ces seules balises.

**Texte depuis Aegisub** : si les lignes sélectionnées contiennent du texte, AegiSignTrack demande au
lancement s'il faut l'importer. *Texte importé* : chaque ligne garde son texte et son style, est suivie
sur sa propre durée et remplacée par ses versions trackées (l'originale est mise en commentaire), toutes
sont placées à l'endroit cliqué sur la vidéo. *Texte saisi ici* : les lignes ne servent qu'à délimiter la
vidéo, et le texte tapé dans l'onglet Texte est ajouté à côté d'elles, sans les modifier. Le choix se
change à tout moment dans l'onglet Texte.

Le cache et le texte ont chacun leur liste *Ce qu'il suit du tracking* : déplacement, agrandissement,
rotation, perspective, opacité. Un texte peut par exemple suivre seulement le déplacement pendant que le cache
suit tout. Les deux peuvent être exportés ensemble.

Raccourcis : <- -> frame précédente / suivante, Espace lecture, Ctrl+Z annuler, Ctrl+Maj+Z rétablir,
H masquer ou revoir les tracés, molette zoom, clic droit (ou clic molette) maintenu pour déplacer la vue.

**Objet qui apparaît d'un coup** : se placer sur une frame où il est bien visible,
dessiner la zone, puis *◀ Tracker* pour remonter le temps.

**Objet qui grossit beaucoup ou sort de l'image** (mode Zone) : quand trop de points sont perdus, le
tracker en reprend automatiquement de nouveaux dans la partie de la zone encore visible et continue le
même mouvement. Une zone presque entièrement sortie de l'image continue sur sa lancée.

**Aplats de couleur, objets sans détails** : en mode Zone, dessiner la zone sur les bords de l'objet
(ou juste autour). Le tracker aligne alors les contours de la zone sur ceux de la frame de départ, ce
qui marche même sans texture. Le canal *Automatique* (par défaut) choisit le mélange de couleurs où la
zone ressort le mieux : un objet de même luminosité que le décor reste suivi. Le bouton *Voir*, à côté de
*Canal*, affiche la vidéo comme le tracker la voit, en niveaux de gris du canal choisi.

**Objet caché ou sorti de l'image** (mode Zone, *Recaler* coché) : quand l'objet passe derrière autre
chose, le tracker prolonge sa trajectoire (déplacement et rotation) et le reprend dès qu'il reparaît.
S'il reste perdu plus de quelques frames, il est recherché dans toute l'image, dans une orientation et
une taille proches de celles attendues. Les frames ainsi estimées sont en orange sur la timeline et
comptées dans le message de fin : à vérifier, et à corriger à la main si besoin. Un objet identique
qui passe par-dessus la cible peut tromper la reprise.

**Suivi qui se décale peu à peu** : *Recaler sur la frame de départ* (coché par défaut, section
Recherche) réaligne chaque frame sur celle où le tracking a commencé, pour que les petites erreurs ne
s'additionnent pas. À décocher si l'objet change vraiment d'aspect en cours de route. Le bouton *Affiner* fait la même
chose après coup sur toutes les frames suivies de la plage : utile sur un tracking déjà fait, ou après avoir replacé
la sélection à la main.

**Plusieurs objets** : la rangée *Objet 1, Objet 2, +* en haut des réglages. Chaque objet a son tracking,
son cache et son texte ; les autres restent visibles en contour gris, et tous sont exportés ensemble.

**Vérifier et corriger un tracking** : le menu *Vue* propose *Verrouiller la vue sur l'objet* (l'objet
reste fixe à l'écran, on voit tout de suite s'il glisse) et la *Courbe de confiance* au-dessus de la
timeline. *◀ estimée* et *estimée ▶* sautent d'une frame orange à l'autre. Après avoir replacé à la main
une frame orange, *Interpoler* refait un passage régulier entre les frames sûres, et *Affiner* le recale.

**Zone à ignorer** (mode Zone, avant de tracker) : entourer une partie de la zone qui s'anime ou
n'appartient pas à l'objet ; elle ne compte plus dans le suivi.

**Texte qui se déforme avec l'objet** : avec *Perspective et déformation* coché dans ce que suit le texte,
l'export écrit `\fscx` et `\fscy` séparément, puis `\frx`, `\fry`, `\fax` et au besoin `\org` : le texte
s'écrase, s'étire et se couche comme l'objet. Il faut que le tracking ait suivi cette déformation, donc
*Cisaillement* ou *Perspective* coché dans l'onglet Tracking. L'aperçu du texte
est rendu par libass (via ffmpeg), y compris pendant la lecture : c'est ce qu'Aegisub affichera, à la
police près si le style n'est pas connu. Limite : un texte sur plusieurs lignes (`\N`) est un peu moins juste.

**Texte découpé par le cache** : dans l'onglet Texte, choisir *Visible seulement dans le cache* (`\clip`)
ou *Caché par le cache* (`\iclip`). Pour faire passer un texte derrière un personnage, dessiner le cache
sur lui : le cache suit le tracking et n'a pas besoin d'être exporté.

**Moins de lignes** : *Regrouper les mouvements réguliers* (menu *Options*) remplace, pour le texte, une
suite de frames au mouvement régulier par une seule ligne avec `\move` et `\t`.

**Avec Aegisub-Motion** : *Options > Données de tracking* copie le suivi de l'objet affiché au format
« After Effects Keyframe Data ».

**Image couchée sur un panneau** : si le cache a exactement 4 points, *Poser l'image sur les 4 points du
cache* place les coins de l'image dessus, dans l'ordre des clics (haut gauche, haut droit, bas droit, bas
gauche) ; l'image est alors en perspective dès la première frame.

**Objet qui s'écrase, s'étire ou se voit de biais** : cocher *Cisaillement* (ou *Perspective*) avant de
tracker. Sans cela le suivi décroche dès que l'objet se déforme, et le message de fin le signale.

**SVG** : *Choisir une image...* accepte aussi un `.svg`. Ses formes sont reprises telles quelles, sans
vectorisation ni perte ; le texte et les dégradés du SVG ne sont pas repris.

**Couleur du cache qui suit l'éclairage** : après avoir pris la couleur avec la pipette, cocher *Reprendre
la couleur à chaque frame* ; elle est relevée au même endroit de l'objet sur chaque frame.

**Réglages et session** : les réglages sont enregistrés à la fermeture et à l'export, puis retrouvés quand
on rouvre le même script Aegisub (ou la même vidéo hors Aegisub). Le travail lui-même (objets, zones,
caches, tracking) est enregistré aussi : en rouvrant la même vidéo sur la même plage, AegiSignTrack
propose de le reprendre ou de repartir de zéro. L'image d'un cache n'est pas conservée, et une session
vieille de plus de 30 jours est effacée.

**Changements de plan** : lancé depuis Aegisub, le tracker reçoit ses keyframes, affichées en traits blancs
sur la timeline, et le tracking s'y arrête au lieu de traverser un cut. *Options > S'arrêter aux
changements de plan* pour le laisser passer.

**Sign qui sort de l'image ou se fond** : régler la plage sur les frames à couvrir, puis *Avancé >
Extrapoler*. Les frames non suivies avant et après le tracking sont remplies en prolongeant son
mouvement ; elles sont en orange, comme les frames estimées.

**Courbes du tracking** (menu *Avancé*) : une fenêtre montre la position, la taille et la rotation dans
le temps. On peut y lisser une seule composante (la rotation qui tremble, par exemple, sans toucher à la
position) et retoucher au chiffre près la frame affichée. Fenêtre fermée et curseurs à 0, rien ne change.

**Tracking fait ailleurs** : *Avancé > Importer des données de tracking...* lit un fichier « After Effects
Keyframe Data » venu de Mocha, d'After Effects ou de Blender (position, taille, rotation, ou les 4 coins
Power Pin) et l'applique à l'objet affiché à partir du début de la plage. Le cache, les dégradés et le
texte s'utilisent ensuite comme après un tracking normal.

**Fondre le cache et le texte dans l'image** : *Mesurer*, à côté du flou, relève le flou des bords de
l'objet sur la frame affichée et règle le flou en conséquence. *Marge* élargit ou rétrécit le cache de
quelques pixels sans le redessiner. Pour un texte à contour, *Flouter aussi l'intérieur des lettres*
exporte chaque ligne deux fois (le contour dessous, les lettres sans contour dessus), parce qu'en ASS le
flou seul ne touche que l'extérieur du contour.

**Autres sorties** (menu *Options*) : *Enregistrer en .ass...* écrit un fichier complet, en-tête et styles
compris. *Données Power Pin* copie les 4 coins du cache frame par frame, pour les scripts de perspective.

**Couleurs de la timeline** : vert, frame suivie ; orange, frame estimée (objet perdu ou caché, position
prolongée) ; gris, frame où l'objet est recouvert. Le vert et l'orange sont exportés, le gris ne l'est pas.

**Objet recouvert peu à peu** (page qui se tourne, volet, personnage qui passe devant) : sous *Autres
suivis*, régler *Recouvrement*. AegiSignTrack compare les couleurs de la zone à celles de la frame de
départ, coupe le cache et le texte le long du bord qui les recouvre (un `\clip` par frame), et n'exporte
plus l'objet quand il est entièrement recouvert : ces frames sont grises sur la timeline. Sur la vidéo, le
contour de la zone montre la partie encore visible, et « recouvert » quand il n'en reste rien.
*Passage* : ce qui est recouvert peut revenir. *Définitif* : ce qui a été recouvert le reste, même si
quelque chose de ressemblant apparaît ensuite au même endroit (la page suivante d'un livre, par exemple).
*Tolérance* règle l'écart de couleur accepté. Limites : mode Zone seulement ; le bord est supposé droit ;
un fondu ou un éclairage qui change est pris pour un recouvrement ; un texte réglé sur *Caché par le
cache* n'est pas coupé (mais il disparaît bien quand l'objet est recouvert).

**Mouvement sur un seul axe** : sous *Translation*, *Axe* vaut *Libre* par défaut. *Horizontal* ou
*Vertical* interdit tout déplacement dans l'autre sens : le centre de l'objet y reste figé, sans le
moindre tremblement, y compris quand l'objet est caché et que sa trajectoire est prolongée. À réserver
aux mouvements vraiment droits (générique qui défile, panoramique) : si l'objet bouge aussi dans l'autre
sens, le tracking sera faux. Pour un mouvement purement droit, décocher aussi *Échelle* et *Rotation*.

**Calques** : *Calque +* (onglets Cache et Texte) s'ajoute au numéro de calque de la ligne d'origine.
Avec 0 pour le cache et 1 pour le texte, le texte reste devant le cache même si les lignes sont triées
ou déplacées dans Aegisub. À 0 partout, rien ne change par rapport à avant.

**Comparer avec l'image d'origine** : la touche H (ou *Vue > Masquer les tracés*) n'affiche plus que la
vidéo. *Vue > Lecture en boucle sur la plage* fait repartir la lecture au début de la plage.

**Loupe** (menu *Vue*) : un agrandissement de la vidéo autour du pointeur, pour poser un point au pixel près.

**Langue** : *Options > Langue / Language*, français ou anglais. Au premier lancement, c'est la langue
du système qui décide. Le plugin Aegisub suit ce choix au démarrage suivant d'Aegisub.

**Mises à jour** : au lancement, AegiSignTrack regarde si une release plus récente existe sur GitHub et
affiche la version installée et la nouvelle, avec un bouton vers la page de téléchargement. Rien n'est
installé tout seul. Il faut `curl` (fourni avec Windows 10 et 11). Se désactive dans le menu *Options*.

**Frames immobiles** : tant que le cache ou le texte ne bouge pas (à 0,15 px près), l'export ne fait
qu'une seule ligne qui dure tout ce temps, au lieu d'une ligne par frame. Décocher *Fusionner les
frames immobiles* (menu *Options*) pour garder une ligne par frame.

**Poids de l'export** : un dégradé ajoute une ligne par bande et par frame (16 bandes par défaut), une
image une ligne par couleur et par frame, chacune contenant tout le contour de cette couleur. L'image
est réduite à 200 px de côté et à quelques couleurs avant d'être convertie en formes, sujet à changements
mais je ne pense pas que vous être prêts à vous retrouver avec un .ass de 1Go et Aegisub qui lag comme
pas possible.

**Couleurs** : *Matrice couleur* (en haut des réglages) force BT.709 ou BT.601 si l'image ou la pipette
ne donnent pas les bonnes couleurs.

**Fondus** : cocher *Opacité* (sous *Autres suivis*, onglet Tracking) avant de lancer le tracking, en partant d'une frame où la zone
est bien visible. Le tracker garde alors la sélection en place quand elle devient invisible, et coche
*Opacité (fondus)* pour le cache et le texte, qui reçoivent `\alpha` frame par frame. Cette case se
règle aussi séparément pour chacun, dans sa liste *Ce qu'il suit du tracking*.

**Résultat différent d'une machine à l'autre** : comparer d'abord la version affichée dans le titre de
la fenêtre (date et commit). Si elle est la même, refaire le tracking avec les mêmes gestes sur les deux
machines, cliquer sur *Options > Diagnostic...* et comparer les deux fichiers : version de ffmpeg,
empreintes des images décodées, réglages, sélection, puis le résultat frame par frame.

## Tests

```sh
cargo test --release
```

Les tests couvrent le tracking, l'export ASS et l'extraction vidéo. Ils utilisent ffmpeg.

## Licence

CC BY-NC-SA 4.0 - Copyright 2026 AnnoyedDev. Usage commercial interdit, toute version modifiée qui est
diffusée doit l'être sous la même licence. Texte complet dans `LICENSE`.
