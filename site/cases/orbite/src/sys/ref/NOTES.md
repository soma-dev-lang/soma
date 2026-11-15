# ref — notes d'architecture

## Conception
- 8 cells, 6 fichiers (`RefBalayage` dans registre.cell) : `RefEtat` (etats), `RefOrg` (organisations), `RefObjet` (catalogue), `RefRegistre` (registre 1975), `RefCession` (cessions), `RefModification` + `RefHistorique` (historique).
- 6 machines : `adhesion` (réactive : tiers/candidat/participant/suspendu), `organisation` (→ radiee), `vie` de l'objet (→ rentre | fragmente), `entree` du registre (→ radie), `cession` (→ realisee | refusee | annulee | expiree), `demande` de modification (→ appliquee | rejetee | annulee | expiree).
- Un État est d'abord « tiers » (connu, non participant) : on catalogue les objets d'États non membres. Objet non attribué : propriétaire provisoire ORG-ORBITE, État "". `amorcer` fonde le système (État du siège admis d'emblée, ORG-ORBITE) : seul geste sans quatre yeux, par "systeme".
- Une seule source par donnée : l'État d'immatriculation vit dans RefRegistre, le propriétaire dans RefObjet ; tutelle d'une org immuable (changer d'État = nouvelle org + cession).
- Propulsion nominale / actuelle : une baisse se déclare seule et vaut tout de suite ; une hausse passe par RefModification à quatre yeux. `propulsion()` rend "aucune" hors en_service ; `peut_manoeuvrer` = en_service ∧ propulsion ≠ aucune ∧ ¬ApiEvenements.en_anomalie (propriété 2 ; le commentaire de façade omet l'anomalie).
- Toute perte de capacité (hors service, cimetière, rentrée, fragmentation, baisse de propulsion) appelle `rouvrir_designations` : ApiCoordination.rouvrir sur les rapprochements où l'objet est désigné (propriété 1).
- Registre : 60 jours après catalogage ; « en_retard » interdit la mise en service ; débris/inconnu « dispense », rouvert par `attribuer` quand l'objet est identifié.
- Cession : demande du cédant → acceptation du cessionnaire → visa État d'immatriculation → visa État du cessionnaire → réalisation à la date d'effet ; une seule en cours par objet, 90 jours max.
- RefHistorique (valeur_au) reconstitue propriétaire, propulsion, État, régime à l'instant d'une décision (propriété 6).

## Règles entre machines (internes, dans les cadres)
- `RefOrg inscrite -> active` when `RefEtat.participant via tutelle_de` (inscrire, activer_tutelle).
- `RefObjet catalogue|hors_service -> en_service` when `RefRegistre.immatricule || a_immatriculer` (même id).
- `RefRegistre * -> radie` when `RefObjet.rentre || RefObjet.fragmente` (même id).
- `RefCession demandee -> acceptee` when `RefOrg.active via cessionnaire_de` ; `acceptee -> visee_cedant` when `RefEtat.participant via etat_cedant_de` ; `visee_cedant -> visee_cessionnaire` when `RefEtat.participant via etat_cessionnaire_de` ; `visee_cessionnaire -> realisee` when `RefObjet.en_service || hors_service via objet_de`.

## Règles entre sous-systèmes proposées
- Cible = RefObjet.rentre ; exige = reentrees: campagne de l'objet constatée ; lien = objet (même id) ou slot `causes`.
- Cible = RefObjet.fragmente ; exige = evenements: fragmentation/collision déclarée ; lien = slot `causes` (id d'évènement).
- Cible = RefObjet.retire ; exige = fin_de_vie: mission de retrait autorisée (propriétaire ET État) puis capture ; lien = slot `causes` (id de mission).
- Cible = RefOrg.radiee ; exige = operateurs: statut retire | exclu (ou jamais candidat) ; lien = org (même id).
- Cible = RefCession.realisee ; exige = manoeuvres: aucun plan approuvé non vérifié sur l'objet ; lien = objet_de.
- Pour les autres : ManPlan.approuve exige ref:RefObjet.en_service (via objet du plan) ; LanLancement.identifie exige RefRegistre.a_immatriculer|immatricule ; FdvMission.capture exige RefObjet.hors_service|cimetiere ; Coord.arretee (objet désigné) exige RefObjet.en_service.

## Getters de façade qui manquent
- Aucun après le contrat v2 (voir Phase 1b).

## Points ouverts
- `changer_etat_objet(cible)` route vers six signaux internes (cible "catalogue" : EtatInvalide) ; un État retiré garde ses objets immatriculés et ses organisations actives (operateurs décide).

## ⚠ de verify (aucun ✗)
- « runtime-checked » : bornes `size <=` et invariants entre slots, écrits par des trous.
- « open obligation … via » (tutelle_de, cessionnaire_de, etat_cedant_de, etat_cessionnaire_de, objet_de) : écriture unique à prouver sur les corps de `inscrire` / `demander`.
- `ensures` des getters de type String (statut, etat) : Soma ne prouve que les résultats Int.

## Phase 1b — contrat v2
- Façade : 30/30 signaux branchés (facade.txt). Nouveaux internes : RefOrg.orbite / organisations / changer_tutelle, RefObjet.attribuer (signature v2) / changer_regime / valeur_au / catalogue, RefRegistre.immatriculer (sans document ; cote ONU → consigner_onu) / est_immatricule, RefBalayage.balayer → RefRegistre.constater_retards + RefCession.balayer + RefModification.balayer.
- Horloge : ref n'avait ni every ni now ; les échéances (immatriculation, cessions, modifications) sont toutes jugées dans balayer(h).
- Trous fermés : radiation d'org exige ApiAcces.actifs_de == 0 ; fragmentation exige ApiEvenements.existe + objet ∈ ApiEvenements.objets ; rentrée exige ApiReentrees.constatee ; retrait exige ApiFinDeVie.mission_capturee ; peut_manoeuvrer lit ApiFinDeVie.passive ; declarer_propulsion accepte "systeme" (passivation) et rouvre toujours à "aucune" ; designe(r, "systeme").
- Tutelle : tutelle_de (inscription) reste le lien immuable de la règle d'activation ; `tutelles` porte l'État courant (etat_de) ; le régime ne change plus par RefModification (champ retiré) mais par changer_regime.
- RefBalayage est placé dans registre.cell : app.cell liste les fichiers un à un et je n'y touche pas.

## Phase 1b — demandes
- Ordre d'appel : reentrees doit marquer la rentrée constatée, et evenements créer l'évènement, AVANT d'appeler ApiRef.changer_etat_objet (sinon NonEligible / Inconnu).
- Façade : genre renvoie "capteur" (RefGenreOrg aligné). Refus à recopier : attribuer + Incoherent, Plein ; changer_etat_objet + NonEligible, MotifRequis ; immatriculer + Doublon, MotifRequis.

## Phase 2 — règles entre sous-systèmes (ref + acces)
- Écrites (8, forme arête sauf mention) : RefObjet.rentre ⇐ ReeCampagne.rentree||close via rentree_de (4 arêtes) ; RefObjet.fragmente ⇐ EvtEvenements.confirme||clos via fragmentation_de (4 arêtes) ; RefObjet.retire ⇐ FdvRetrait.capturee via retrait_de (2 arêtes) ; RefObjet.cimetiere ⇐ FdvFin.passive||en_retard (même id) ; AccesPersonnes.active (inconnue→active) ⇐ RefOrg.inscrite||active via employeur_de ; AccesCampagnes.ouverte ⇐ RefOrg.inscrite||active via org_de ; toml (sys/acces/regles.toml, arête déjà prise) : AccesRattachements.confirme ⇐ while RefOrg.inscrite||active via employeur_du ; AccesUrgences.ouverte ⇐ while ConjRapprochement.alerte||en_coordination via rapprochement_de.
- verify (copie de travail + regles.toml fusionné, 2 passes) : cimetiere ✓ (54 états produits) ; les 7 autres « open obligation … written by the hole » (lien écrit par un trou) ; aucun ✗ ni « not explored » sur ref/acces (seul ✗ global : EscVeille, pas à nous). cycles.py : aucune composante, profondeur max 23.
- Écartées : RefOrg.radiee ⇐ OpMembres retire|exclu|jamais candidat (3 alternatives dont l'état initial pour les orgs non opérateurs ; tenu au corps par ApiOperateurs.statut) ; RefCession.realisee ⇐ aucun plan approuvé non vérifié (négation sur un ensemble de plans, pas un état) ; RefObjet.en_service ⇐ LancPiece.identifiee|immatriculee (arête déjà gardée par RefRegistre, lien origine_de chez lancements, objets hors lancement) ; objet catalogué ⇐ ObsPistes.cataloguee (état initial = création, pas d'arête ; plusieurs créateurs) ; propulsion non redéclarable après passivation (pas un état : RefObjet.modifier appelle ApiFinDeVie.passive, refus NonEligible) ; cimetiere depuis FdvFin.defaillant (3e alternative écartée : coût de verify ; le requires toml « passé par en_retrait|defaillant » sortait ✗ sur le trou).
- Structure : slot `causes` remplacé par rentree_de / fragmentation_de / retrait_de (un écrivain chacun, immuables ; cause() lit les trois) ; calls ajoutés : placer_cimetiere + ApiFinDeVie.passive/en_retard, modifier + ApiFinDeVie.passive, AccesRattachements.ouvrir/confirmer + ApiRef.organisation_existe ; commentaires de refus mis à jour.
- ORDRE (phase 3) : reentrees passe la campagne en « rentree », evenements confirme l'évènement AVANT ApiRef.changer_etat_objet ; la façade doit fournir la référence (id campagne / évènement / mission) aux signaux internes. Getters manquants : ApiReentrees.campagne_de(objet), ApiFinDeVie.mission_de(objet), ApiEvenements.confirme(ev), état defaillant de FdvFin.

## Phase 3 — corps
- Signaux : 124/124 remplis (8 cells + façade ApiRef), ~1 560 lignes de corps (sys/ref/chair/*.cell, 7 fichiers) ; banc_ref.cell `check` sans erreur.
- Tests : test_ref.cell, 17 tests / 305 assertions, tous verts (`$S test test_ref.cell`) — 6 machines de bout en bout, refus d'habilitation / quatre yeux / portée / état, scénario métier « nouvel État → opérateur → lancement → cession → perte de propulsion → rentrée → radiation ». Doublures : doublures/ref.cell (ApiAcces, ApiOperateurs, ApiFinDeVie, ApiReentrees, ApiEvenements, ApiConjonctions, ApiCoordination, ApiPasserelle ; pures : objets témoins OBJ-3/4/5, personnes "role#n@ORG-X").
- Cadres amendés (--amend, check app.cell OK) : RefEtat.admettre/rejeter/retablir `reads noms` ; suspendre/retirer `reads noms, instruments` (l'État du siège = instrument "ORBITE:fondation", écrit par fonder seul, refusé par candidater). Invariants à deux slots : `(admis_le ?? declares_le ?? 0) >= (declares_le ?? 0)` (RefEtat) et `(immatricules_le ?? ouvertes_le ?? 0) >= (ouvertes_le ?? 0)` (RefRegistre) — l'ancien `?? 0` refusait TOUTE déclaration d'État (declarer) et TOUTE ouverture d'entrée (ouvrir, donc tout catalogage) dès h > 0.
- Demandes à l'intégrateur : (1) getter ApiEvenements.confirme(ev) dans les calls de RefObjet.constater_fragmentation (la règle EvtEvenements.confirme||clos n'est pas vérifiable au corps : seuls existe + objets le sont) ; (2) ApiReentrees.campagne_de(objet) et ApiFinDeVie.mission_de(objet) dans constater_rentree / constater_retrait, pour comparer `reference` au lien (aujourd'hui : constatee / retrait_autorise+mission_capturee par objet) ; (3) façade ApiRef plus large que ses corps (avertissements) : retirer de cataloguer `RefRegistre.ouvrir`, de declarer_propulsion `RefObjet.rouvrir_designations`, de valeur_au `RefHistorique.valeur_au`, de attribuer `RefRegistre.requalifier`, de balayer `RefRegistre.constater_retards, RefCession.balayer, RefModification.balayer` (appelés par les internes).
- Défauts de conception : un débris attribué garde l'État "" au registre (requalifier ne s'applique qu'hors débris ; aucun signal n'écrit l'État d'un débris dispensé) → etat_immatriculation "" et cession impossible ; RefRegistre.immatriculer : « analyste pour un État non participant » approché par « État sans autorité inscrite » (RefEtat.participant hors calls) ; RefCession.demander accepte un objet en cimetière mais realiser l'exige en_service|hors_service (il expire) ; changer_etat_objet ignore `motif` pour retire/rentre/fragmente (motif interne = référence).
- Trous laissés : aucun.
- Limites de Soma : piège 1 (`require !a || b`) rencontré dans cataloguer, corrigé, et parenthésé aussi dans un `if !a && b` ; types nominaux (Etat, Org, Objet, RefChamp…) : to_string / Org(…) à chaque frontière d'appel et d'écriture de slot ; invariant entre slots : `?? 0` refuse la première écriture (d'où les amendements), et `check` garde un avertissement sur la forme corrigée ; doublures sans mémoire → comportements lus sur les identifiants.
