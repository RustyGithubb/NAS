# Changelog

## Non publié
### Ajouts
- Tableau de bord : panneau Réseau (Wi-Fi, signal, bande et canal, liaison, point d'accès, IP, passerelle, DNS, débit reçu/envoyé en direct sur 5 min, latence et pertes vers la box et internet, coupures Wi-Fi et leur raison, bouton « Tester le débit »), en rouge si signal faible, pertes ou coupure récente. Le stockage passe à droite. Nécessite la mise à jour de l'agent (sinon « Agent trop ancien »).
- Fichiers : disques connectés listés sous « Ce PC » dans le volet de gauche (nom Windows du disque, clé USB, lecteur réseau…), comme dans l'Explorateur.
- Fichiers : vignettes des images en vue « Grandes icônes » et dans la Galerie, à la place de l'icône.
- Fichiers : un onglet tiré par son intitulé hors de la barre d'onglets s'ouvre seul dans sa fenêtre (barre de titre maison, épingle « toujours au premier plan ») ; la fermer le ramène dans l'explorateur. Chaque fenêtre affiche son propre dossier ; copier/coller marche d'une fenêtre à l'autre.
- Fichiers : aperçu du fichier sélectionné dans le volet Détails à la place de l'icône (images png, jpg, gif, webp, bmp jusqu'à 25 Mo ; début des fichiers texte et de code, 64 Ko).
### Corrections
- Fichiers : volet de gauche réorganisé, groupe « NAS » avec Accueil, Galerie (dossier Médias), Fichiers (dossier Partage) et Sauvegardes.
- Agent : les départs volontaires du Mac (reconnexion, réglage Wi-Fi changé) ne comptent plus comme des coupures dans le panneau Réseau.
- Console : un agent d'une autre version du protocole n'est plus traité comme hors ligne ; Réglages › Serveur garde le bouton « Rechercher » pour le mettre à jour, et les onglets expliquent quoi faire.
- Fichiers : plus d'espace autour des séparateurs du volet de gauche ; boutons couper / copier / coller / renommer / supprimer retirés de la barre du haut (toujours au clic droit et au clavier).
- Fichiers : glisser un élément d'un dossier du serveur à un autre le déplace (Ctrl : copie) au lieu de le dupliquer.
- Fichiers : volet de navigation plus serré (lignes plus basses, sans espace entre elles).
- Fichiers (Windows) : glisser des éléments hors de la console les dépose dans l'Explorateur ou sur le Bureau. Ceux du serveur sont téléchargés dès que le pointeur sort de la fenêtre (visibles dans Transferts) ; l'Explorateur les copie une fois le téléchargement fini.
- Fichiers : « Ce PC » dans le volet de gauche (dossiers personnels et disques de cet ordinateur : ouvrir, renommer, nouveau dossier, corbeille de Windows, épingler). Glisser-déposer d'un dossier à un autre (liste, autre onglet — survolé, il passe au premier plan —, volet de gauche) : serveur → PC = téléchargement, PC → serveur = envoi, serveur → serveur ou PC → PC = copie (Maj : déplacement) ; dossiers compris. Copier/coller marche aussi entre le serveur et ce PC. « Télécharger… » demande le dossier de destination. Onglet Transferts : progression, vitesse, temps restant, ✓ ou ✗, Ouvrir / Afficher dans le dossier / Afficher sur le serveur, « Effacer les terminés ».
- Fichiers : bouton ⧉ pour détacher l'explorateur dans sa propre fenêtre (même barre de titre, épingle « toujours au premier plan », position et taille retenues) ; fermer la fenêtre le ramène dans la console.
- Fichiers façon Explorateur Windows : onglets (Ctrl+T / Ctrl+W, clic du milieu), barre d'adresse avec recherche dans le dossier, barre de commandes (Nouveau, couper, copier, coller, renommer, supprimer, Trier, Afficher, Détails), volet Accueil / Galerie / épinglés / emplacements, page Accueil (accès rapide, fichiers récents), page Galerie, volet Détails ; menus clic droit sans raccourcis. Protocole v9.
- Journaux : vue « Tout le système » par défaut, comme `journalctl -f` dans un terminal (invite, heure, machine, programme en couleur, erreurs en rouge, avertissements en jaune) ; services à surveiller toujours visibles, tous les services dans un menu dépliable. Protocole v8.
- Mise à jour automatique de l'agent depuis GitHub (chaque nuit, seulement si la CI est verte, retour à l'ancienne version si la nouvelle ne démarre pas) et bouton « Rechercher » dans Réglages › Serveur. Protocole v7.
- `deploy/` : services systemd (démarrage de l'agent, mise à jour nocturne, rallumage du Mac mini après une coupure de courant) et `install.sh`.
- Problèmes en rouge partout : services en échec ou arrêtés alors qu'ils démarrent avec le système (filtre « Problèmes »), tuiles et jauges du tableau de bord au-delà de 90 % (ou 85 °C), sites coupés, erreurs. Les tâches ponctuelles arrêtées restent grises. Protocole v6.
- Tableau de bord : espace disque comme `df` sous Linux (utilisé sans la réserve de l'administrateur, et espace disponible). Protocole v5 : mettre à jour l'agent et la console ensemble.
- Squelette du projet : console (Windows), agent (serveur) et données partagées.
- Console : fenêtre sans cadre, barre latérale flottante (Tableau de bord, Services, Journaux, Fichiers, Réglages), 12 thèmes, français / anglais.
- Agent : envoie l'état de la machine (processeur, mémoire, disques, température, durée de fonctionnement).
- Tableau de bord : affiche cet état en direct ; adresse de l'agent dans Réglages › Serveur.
- Services : liste avec recherche et filtres, démarrer / arrêter / redémarrer (simulés hors du serveur).
- Journaux : lignes d'un service en direct, filtre, couleurs selon la gravité.
- Clé d'accès : l'agent refuse toute demande sans sa clé ; la console la garde dans le coffre de mots de passe de Windows (Réglages › Serveur).
- Repris de WorkTable : réglages des animations (vitesse, opacité, courbe, aperçu), police et taille par groupe de textes (titres, interface, texte, code), roue du logo qui fait trembler les cartes ; Réglages organisés comme WorkTable.
- Sites : sites web hébergés (WordPress, pages fixes) — en ligne / maintenance / coupé, certificat HTTPS, dossier des médias, sauvegarde immédiate (simulés hors du serveur).
- Onglets uniformisés comme WorkTable : même en-tête (sur-titre, titre, info), panneaux pleine hauteur, colonne de gauche de 260 px ; Services en trois colonnes (filtres, liste, détail), Journaux et Sites en liste + détail.
- Éléments en échec sur fond teinté d'accent (avec liseré), éléments arrêtés ou coupés « creusés » (fond sombre, contour, texte estompé).
- Journaux façon Terminal Windows (couleurs « Campbell ») : barre d'onglets avec le service ouvert, terminal qui occupe tout le panneau, curseur clignotant.
- Tableau de bord sur une seule grille : même écart entre tuiles, entre tuiles et panneaux, et entre panneaux ; panneaux alignés sur les tuiles.
- Tableau de bord : processeur sur toute la largeur (cœurs en grille équilibrée), stockage en dessous avec l'espace libre ; jauges sur toute la largeur disponible.
- Fichiers : explorateur des emplacements du serveur (Médias, Partage, Sauvegardes) en liste ou en galerie avec des émojis par type ; précédent / suivant / parent, fil d'Ariane, tri par colonne, sélection multiple (Ctrl, Maj), renommer (F2), nouveau dossier, copier / couper / coller, corbeille, favoris, menu clic droit, raccourcis clavier.
- Fichiers : recherche instantanée dans tout le serveur façon Everything (`*.png`, `ext:jpg;png`, `dossier:`), index tenu par l'agent.
- Fichiers : envoi par glisser-déposer ou bouton « Envoyer… », téléchargement dans Téléchargements\NAS (double-clic = télécharger et ouvrir).
