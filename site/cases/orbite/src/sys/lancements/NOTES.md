# lancements — notes d'architecture
**Découpage** (6 cells, 6 machines, clé = id du lancement sauf LancPiece) : `LancLancement` (préavis → tir →
injection → identification ; reports, échec, annulation), `LancCola` (criblage de la fenêtre, valable 8 h),
`LancAvis` (avis d'ORBITE à quatre yeux), `LancZones` (États de lancement + zones de retombée notifiées),
`LancPiece` (clé = pièce du manifeste : séparation, catalogage, identification, immatriculation, délais),
`LancEnquete` (échec : débris en orbite, retombées, États notifiés) ; `LancVeille` (sans machine) change en
états les délais dépassés, avec le `h` de l'ordonnanceur (jamais `now()`).
**Choix** : l'avis d'ORBITE n'autorise pas (l'État autorise) ; « autorise » = avis favorable en vigueur. Un tir
est un FAIT : il s'enregistre toujours — sous avis (`confirmer_tir`, seulement sous avis favorable et hors
périodes interdites) ou hors avis (`constater_tir_hors_avis`, manquement gravité 3), et un lancement jamais
notifié (État non participant, préavis < 72 h) par `enregistrer_non_notifie` (quatre yeux). Un COLA périmé
(âge, tempête, nuage) rend l'avis caduc et ramène le lancement à « notifie ». Un objet découvert par
l'observation peut être rattaché à une pièce (`cataloguer` avec objet ≠ "") ; un élément imprévu s'ajoute au
manifeste après injection. Les données d'autrui sorties d'un COLA passent par `ApiDonnees.peut_recevoir`
(sinon anonymisées) et sont tracées. L'avis garde le COLA qui le fonde (`LancFondement` : version, instant,
trajectoire, dépôts d'éphémérides, facteur météo, interdits) : propriété 6. Pièces neuves = « catalogue »
au référentiel, donc non désignables pour manœuvrer tant que le propriétaire ne les met pas en service (propr. 2).
**Règles entre machines posées dans les cadres** (verify : 43 ✓, aucun ✗) :
- `LancLancement notifie -> crible when LancCola.conclu` ; `crible -> autorise when LancAvis.favorable` ;
  `autorise -> lance when LancAvis.favorable` (même id)
- `LancAvis redige -> favorable when LancCola.conclu` ; `a_rediger|caduc -> redige when LancZones.declarees || acquittees`
- `LancPiece prevue -> separee when LancLancement.injecte via lancement_de` (écrit par `prevoir` seul)
- `LancEnquete a_ouvrir -> ouverte when LancLancement.echec || LancLancement.injecte`
**Règles entre sous-systèmes proposées** :
- Cible = LancLancement.autorise ; exige = operateurs:opérateur actif (est_actif) ; lien = op_de
- Cible = LancPiece.cataloguee ; exige = ref:objet catalogué (etat_objet ≠ "inconnu") ; lien = objet_de
- Cible = LancPiece.immatriculee ; exige = ref:objet inscrit au registre de son État ; lien = objet_de
- Cible = ref:objet en_service (objet issu d'un lancement) ; exige = lancements:LancPiece.identifiee || immatriculee ; lien = origine_de (essaim mal attribué ⇒ jamais désigné : propr. 2)
- Cible = manoeuvres:plan approuvé ; exige = lancements:pièce identifiée si l'objet est issu d'un lancement ; lien = origine_de
- Cible = LancEnquete.close ; exige = responsabilite:dossier gelé si dommages au sol ; lien = id du lancement (à exposer)
- Cible = LancPiece.identifiee (genre "etage") ; exige = fin_de_vie:plan_conforme ; lien = objet_de (aujourd'hui manquement, non bloquant)
**Phase 1b (contrat v2)** : 7/7 signaux de façade branchés (facade.txt). Trous fermés : `LancPiece.rattacher`
(ApiRef.attribuer, quatre yeux) ; `immatriculer` écrit le registre chez ref (ApiRef.immatriculer) et la veille
reprend une immatriculation faite directement chez ref (ApiRef.immatricule) ; retard d'immatriculation →
ApiConformite.constater_etat ; COLA refusé ou « impossible » sans ApiConjonctions.campagne_fraiche ; enquête
close avec dommages ⇒ gel (ApiResponsabilite.sous_gel / geler "systeme") ; `etats_de_lancement(objet)` via
LancZones. Horloge : aucun every/now ; `LancVeille.balayer(h, par)` = ApiLancements.balayer.
**Phase 1b — demandes** : aucune bloquante ; `ApiResponsabilite.sous_gel` sur un id de lancement suppose que
`geler(evenement = lancement)` l'accepte comme cible (à confirmer par responsabilite).
**Avertissements** : `len(slot)` voulu ; debut_de ≤ fin_de (écrire fin_de d'abord) ; verify : ⚠ de trous seulement.

## Phase 2 — règles entre sous-systèmes (reentrees + lancements)
Écrites : 7 (verify, toml fusionné dans une copie : aucun ✗ ni « not explored » sur ces cells ; les 7 sortent
« open obligation … written by the hole » = lien écrit par un signal sans corps, jugé au corps).
- ReeControlee.approuve ⇐ ManPlan.crible || ManPlan.approuve via manoeuvre_de — toml (sys/reentrees/regles.toml ; arête déjà prise par ReeCampagne)
- ReeControlee.executee ⇐ ManPlan.execute || ManPlan.verifie via manoeuvre_de — arête (confirmer)
- ReeConstat.transmis ⇐ RespDossiers.gele via dossier — arête (transmettre, seul écrivain de `dossier` ; + calls ApiResponsabilite.sous_gel)
- ReeDebris.identifie ⇐ RefObjet.rentre || RefObjet.fragmente via objet_attribue — arête (identifier, seul écrivain)
- LancLancement.autorise ⇐ OpMembres.actif via op_de — toml (sys/lancements/regles.toml ; arête prise par LancAvis.favorable ; + calls ApiOperateurs.est_actif, refus Suspendu)
- LancPiece.cataloguee ⇐ RefObjet.catalogue via objet_de — arête (nouveau LancPiece.lier)
- LancPiece.immatriculee ⇐ RefRegistre.immatricule via objet_de — arêtes (immatriculer et relever_retards, 4 arêtes)
Structure : LancLancement.creer (nouveau, seul écrivain de op_de, appelé par notifier et enregistrer_non_notifie) ;
LancPiece.lier (nouveau, seul écrivain de objet_de/origine_de, appelé par cataloguer et rattacher : rattacher exige
désormais l'objet « catalogue » chez ref). Aucun état ajouté. cycles.py : aucune composante, profondeur max 23.
Écartées : LancEnquete.close ⇐ dossier gelé si dommages (conditionnelle ; ApiResponsabilite n'expose pas le dossier
d'un évènement et geler refuse Existe ⇒ lien inconnu) ; LancPiece.identifiee (étage) ⇐ plan de fin de vie conforme
(conditionnelle au genre, voulue non bloquante : manquement) ; LancLancement.identifie ⇐ RefRegistre (proposition ref :
pas d'objet unique par lancement ; transposée à la pièce il faudrait ≥ 3 alternatives a_immatriculer|en_retard|
immatricule|dispense). Inverses (cibles chez ref, conjonctions, fin_de_vie, manoeuvres) : hors périmètre.
Verify : 1 seul ✗ global, EscVeille.couverte (escalade, pas à moi).

## Phase 3 — corps (14/11/2026)
- **83/83 signaux remplis** (76 internes + 7 façade), 1 312 lignes de corps (sys/lancements/chair/*.cell, 7 fichiers) ;
  doublures/lancements.cell (13 façades, pures) ; **test_lancements.cell : 18 tests, 318 assertions, tous verts**.
  Couverts : les 6 machines de bout en bout, quatre yeux, rédacteur ≠ valideur, portée opérateur/État, états invalides,
  veille (fenêtre échue, 180 j, COLA > 8 h, tempête, délais 30/60 j, perte), scénario report → nouvelle fenêtre → avis → tir → identification.
- **Cadres amendés** (skeleton --amend, check app 0 erreur, cycles.py : aucun) : `prendre_acte_cola` + calls LancCola.etat ;
  `prendre_acte_avis` + LancAvis.etat (les deux pour `require` la règle de l'arête) ; `clore_identification` +
  LancPiece.manifeste (manifeste non vide) ; `LancPiece.separer` + LancLancement.etat (règle `injecte via lancement_de`) ;
  `LancEnquete.ouvrir` + LancLancement.etat (règle `echec || injecte`).
- **Demandes à l'intégrateur** : aucune bloquante. ApiRef.orbite n'est pas dans les calls : "ORG-ORBITE" écrit en dur
  (catalogage d'une pièce au propriétaire "inconnu", message à la direction d'enregistrer_non_notifie).
- **Défauts de conception** : `nb_objets_de >= 1` contre « 0 pièce annoncée » (non notifié) ⇒ aucune entrée, fiche = 0, et
  `prevoir` laisse l'analyste ajouter les pièces d'un lancement à 0 pièce annoncée et ne demande pas un État participant
  pour un propriétaire "inconnu" ; `declarer_injection` devait demander le suivi des objets rattachés mais n'a pas
  ApiObservation.demander_suivi ; `enregistrer_non_notifie` laisse le COLA « a_cribler » et l'avis « a_rediger » (pas
  d'archiver dans ses calls) et son `writes vivants` est trop large (creer l'écrit) ; `lier` ne lit pas lancement_de
  (pièce inconnue ⇒ EtatInvalide, pas Inconnu) ; `constater_retombee` n'écrit pas retards_de (une pièce retombée y reste) ;
  la version des zones déclarées n'est pas exposée (rediger ne vérifie pas qu'elles couvrent la fenêtre en cours) ;
  `vivants` est une List en ajout : retrait par `vivants.delete(index_of(…))`.
- **Trous laissés** : aucun. **Limites Soma** : une doublure pure ne retient rien (ApiRef.immatriculer ne rend pas
  `immatricule` vrai : convention OBJ-n, n ≥ 5000 ; retombée réussie non testable, l'objet doit être « catalogue » au lien puis « rentre ») ;
  une List slot ne se réaffecte pas (E0064).
