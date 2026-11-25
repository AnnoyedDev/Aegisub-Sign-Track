# AegiSignTrack

### English (one day maybe, for now, use GTranslate or DeepL too lazyfor now sowwy ;_;).

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

Il faut `ffmpeg` et `ffprobe` dans le PATH.

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
molette zoom, clic droit (ou clic molette) maintenu pour déplacer la vue.

**Objet qui apparaît d'un coup** : se placer sur une frame où il est bien visible,
dessiner la zone, puis *◀ Tracker* pour remonter le temps.

**Objet qui grossit beaucoup ou sort de l'image** (mode Zone) : quand trop de points sont perdus, le
tracker en reprend automatiquement de nouveaux dans la partie de la zone encore visible et continue le
même mouvement. Une zone presque entièrement sortie de l'image continue sur sa lancée.

**Aplats de couleur, objets sans détails** : en mode Zone, dessiner la zone sur les bords de l'objet
(ou juste autour). Le tracker aligne alors les contours de la zone sur ceux de la frame de départ, ce
qui marche même sans texture. Le canal *Automatique* (par défaut) choisit le mélange de couleurs où la
zone ressort le mieux : un objet de même luminosité que le décor reste suivi.

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

**Frames immobiles** : tant que le cache ou le texte ne bouge pas (à 0,15 px près), l'export ne fait
qu'une seule ligne qui dure tout ce temps, au lieu d'une ligne par frame. Décocher *Fusionner les
frames immobiles* (en haut) pour garder une ligne par frame.

**Poids de l'export** : un dégradé ajoute une ligne par bande et par frame (16 bandes par défaut), une
image une ligne par couleur et par frame, chacune contenant tout le contour de cette couleur. L'image
est réduite à 200 px de côté et à quelques couleurs avant d'être convertie en formes, sujet à changements
mais je ne pense pas que vous être prêts à vous retrouver avec un .ass de 1Go et Aegisub qui lag comme
pas possible.

**Couleurs** : *Matrice couleur* (en haut des réglages) force BT.709 ou BT.601 si l'image ou la pipette
ne donnent pas les bonnes couleurs.

**Fondus** : cocher *Suivre l'opacité* avant de lancer le tracking, en partant d'une frame où la zone
est bien visible. Le tracker garde alors la sélection en place quand elle devient invisible, et coche
*Opacité (fondus)* pour le cache et le texte, qui reçoivent `\alpha` frame par frame. Cette case se
règle aussi séparément pour chacun, dans sa liste *Ce qu'il suit du tracking*.

**Résultat différent d'une machine à l'autre** : comparer d'abord la version affichée dans le titre de
la fenêtre (date et commit). Si elle est la même, refaire le tracking avec les mêmes gestes sur les deux
machines, cliquer sur *Diagnostic...* (en haut) et comparer les deux fichiers : version de ffmpeg,
empreintes des images décodées, réglages, sélection, puis le résultat frame par frame.

## Tests

```sh
cargo test --release
```

Les tests couvrent le tracking, l'export ASS et l'extraction vidéo. Ils utilisent ffmpeg.

## Licence

CC BY-NC-SA 4.0 - Copyright 2026 AnnoyedDev. Usage commercial interdit, toute version modifiée qui est
diffusée doit l'être sous la même licence. Texte complet dans `LICENSE`.
