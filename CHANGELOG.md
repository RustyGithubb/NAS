# Changelog

## Non publié
### Ajouts
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
