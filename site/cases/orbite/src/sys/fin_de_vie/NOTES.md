# fin_de_vie — notes d'architecture
**Découpage** (7 cells, 7 machines) : `FdvPlan` ("PFV-<n>"), `FdvFin` (objet), `FdvPassivation` (objet),
`FdvConsentement` ("<mission>|<partie>"), `FdvRetrait` ("RA-<n>"), `FdvProrogation` ("PRO-<n>"), `FdvCouverture` (jour).

**Choix** : conformité calculée au dépôt (LEO ≤ 5 ans, GEO cimetière ≥ 235 km sous 1 an, MEO/HEO ≤ 25 ans, habité ⇒
rentrée contrôlée, succès ≥ 90 %, réserve ≥ requis) ; un plan non conforme est accepté mais jamais approuvé.
Reconfirmation annuelle (400 j) sinon caduc. L'échéance est fixée à la fin de mission, ne bouge qu'une fois (prorogation
avis État + quatre yeux direction). Le silence du propriétaire ne bloque rien : `FdvCouverture` déclare la fin d'office
(objet hors service ou fin prévue + 90 j), constate les objets sans plan et balaie les retards (`a_echoir`).
La vérification de passivation déclare la propulsion "aucune" au catalogue (propriété 2 : un objet passivé n'est
jamais désignable). Retrait actif : deux consentements (propriétaire, puis État), autorisation à quatre yeux,
approche = plan approuvé chez manoeuvres (donc criblé, propriété 4), consentements irrévocables à la capture.
Propriété 6 : empreintes, "par|valideur", `dossier`/`consulter*` via `ApiDonnees.peut_recevoir` + `tracer`.

**Règles entre machines (cadres)** : `FdvFin en_retrait -> passive when FdvPassivation.verifiee` et
`FdvPassivation a_planifier -> en_cours when FdvFin.en_retrait || en_retard` (même id) ;
`FdvPlan approuve -> clos when FdvFin.rentre || cimetiere via objet_de` ; `FdvRetrait proposee -> accord_proprietaire`
et `consentie -> autorisee when FdvConsentement.accorde via consentement_proprietaire_de`, `accord_proprietaire ->
consentie … via consentement_etat_de`, `capturee -> achevee when FdvFin.rentre || cimetiere via cible_de` ;
`FdvConsentement accorde -> irrevocable when FdvRetrait.capturee via mission_de` ; `FdvProrogation avis_favorable -> accordee when FdvFin.en_retrait || passive via objet_de`.
**Verify** OK (44 ✓) ; ⚠ : `via` = obligations des trous ; `*.size <= MAX` runtime-checked (corps : `require len < MAX`) ;
entre slots `?? 0` : écrire le slot de droite d'abord ; `len(x_de) <= N` borne voulue de chaque liste.

**Règles entre sous-systèmes proposées** :
- Cible = ref:objet "cimetiere" ; exige = FdvFin.passive || en_retard || defaillant ; lien = objet (même id)
- Cible = ref:propulsion ≠ "aucune" (redéclarée) ; exige = non FdvPassivation.verifiee ; lien = objet
- Cible = coordination:désignation arrêtée ; exige = non FdvPassivation.verifiee de l'objet désigné ; lien = objet
- Cible = manoeuvres:plan "approuve" motif desorbitation/transfert ; exige = FdvFin.en_retrait || en_retard ; lien = objet
- Cible = manoeuvres:plan "approuve" d'un chasseur ; exige = FdvRetrait.autorisee ; lien = à créer (`mission_de` du plan)
- Cible = FdvRetrait.autorisee ; exige = operateurs:servicer actif ; lien = servicer_de
- Cible = FdvPlan.approuve ; exige = ref:objet catalogue || en_service ; lien = objet_de

**Phase 1b** : 8/8 signaux de façade branchés (facade.txt) ; horloge : aucun now/every, tout part de
`ApiFinDeVie.balayer` → `FdvCouverture.balayer` (campagne du jour, 20 opérateurs `ApiOperateurs.actifs` et 100 éléments
par file et par battement : échéances, plans, consentements, prorogations, missions). Trous fermés : cimetière sur
`ApiObservation.perigee/apogee` ; plans rattachés et approche contrôlés par `ApiManoeuvres.objet_de/motif_de` ;
rendez-vous exempté (`ApiConjonctions.exempter` à l'autorisation) ; rentrée à risque (`ApiReentrees.a_risque`) ⇒
désorbitation contrôlée ; passivation vérifiée ⇒ `ApiRef.declarer_propulsion("aucune")` par l'analyste.
**Phase 1b — demandes** : aucune (declarer_propulsion reçoit le `par` de l'analyste vérificateur).
**Points ouverts** : propriétaire "inconnu" (l'État consent deux fois) ; cession après approbation ; sens de ref "retire".

## Phase 2 — règles entre sous-systèmes (manoeuvres + fin_de_vie)
Règles écrites : 10, toutes sous forme d'arête (aucun regles.toml).
- ManPlan, chaîne d'approbation transitoire, prise d'un seul tenant dans `approuver` : crible -> recoupe ⇐ ConjCriblagePlan.sur (même id ; propriété 4) ; recoupe -> coordonne ⇐ CoordDossier.arretee via verrou_de (évitement ; propriété 1) ; recoupe|coordonne -> habilite ⇐ OpMembres.actif via proprietaire_de ; habilite -> localise ⇐ EphSuivi.a_jour via objet_de ; localise -> capable ⇐ RefObjet.en_service via objet_de ; capable -> approuve ⇐ ManEvitement.engage via verrou_de (règle interne, conservée).
- ManCriblage demande -> sur ⇐ EphDepot.validee via depot_de.
- FdvPlan en_revue -> approuve ⇐ RefObjet.catalogue || en_service via objet_de.
- FdvRetrait consentie -> instruite (transitoire, conserve la règle interne du consentement) -> autorisee ⇐ OpMembres.actif via servicer_de ; en_approche -> capturee ⇐ RefObjet.hors_service || cimetiere via cible_de.
- FdvFin * -> rentre ⇐ RefObjet.rentre, * -> fragmente ⇐ RefObjet.fragmente (même id ; remplace « ReeCampagne.rentree », dont l'id de campagne n'est pas connu côté objet).
Structure : états ManPlan recoupe, coordonne, habilite, localise, capable et état FdvRetrait instruite (jamais observés entre deux appels) ; slots ManPlan.objet_de, ManPlan.proprietaire_de (écrits par declarer), ManCriblage.depot_de (écrit par cribler). Getters ajoutés aux calls : ApiConjonctions.criblage_plan, ApiRef.etat_objet (approuver, capturer), ApiEphemerides.depot_valide (cribler). Commentaires et refus mis à jour.
Écartées : ManEvitement.engage ⇐ coordination (les verrous « P: » hors évitement n'ont pas de coordination ; couvert par ManPlan.coordonne) ; ManPlan.declare ⇐ CoordDecision (état initial, pas d'arête) ; désorbitation ⇐ FdvFin en_retrait|en_retard (aucun getter ApiFinDeVie.en_retrait/etat : DEMANDE) ; chasseur ⇐ FdvRetrait.autorisee (pas de mission dans declarer, façade figée) ; verifie ⇐ ObsOrbites.validee (OD inconnue à la création) ; OpsBascule, EvtAnomalies, LancPiece, ReeControlee (pas de lien d'instance, ou id inconnu à la création ; contrôles faits dans le corps).
Verify (1 seul passage) : aucun ✗ et aucun « not explored » sur mes cells ; règles via = « open obligation … written by the hole » ; règles même id comptées dans les ✓ (1026 au total). Le seul ✗ global est EscVeille.couverte (escalade, pas à moi). cycles.py : aucune composante, profondeur max 23.

## Phase 3 — corps
- **Signaux remplis : 79/79** (70 internes + 9 de façade), aucun trou ; ~1 670 lignes de corps (sys/fin_de_vie/chair/*.cell), doublures 282 lignes.
- **Tests** : test_fin_de_vie.cell, 14 tests, 401 assertions, **tous verts** (cycles des 7 machines, refus d'habilitation, portée, quatre yeux, états ; scénario GEO plan → fin → passivation → manœuvres vérifiées → cimetière → plan clos ; retrait actif proposé → 2 consentements → autorisé → approche → capture (irrévocable) → cimetière de la cible → mission achevée ; campagne de couverture par ApiFinDeVie.balayer).
- **Cadres corrigés (amendés, 2 amend)** : 5 invariants entre slots `(x ?? 0) >= (y ?? 0)` refusaient TOUJOURS la première écriture de y (FdvPlan revu_le/depose_le, FdvFin sortie_le/fin_le, FdvPassivation declaree_le/commence_le et verifiee_le/declaree_le, FdvConsentement repondu_le/demande_le) → côté gauche par défaut `?? 999999999999999`. Reads ajoutés : FdvFin.rattacher_manoeuvre `reads fin` ; FdvConsentement.classer/conditions/etat `reads mission_de` ; FdvPlan.etat et prendre_en_revue `reads objet_de` ; FdvRetrait.etat `reads cible_de` ; FdvProrogation.etat `reads objet_de`, demander `reads prorogation` (Doublon) ; FdvCouverture.balayer `reads d_office_de` (compte des changements).
- **Demandes à l'intégrateur** : (1) `ApiFinDeVie.plan_conforme(objet: Objet, h: Horodatage)` — sans h, le corps juge à h = 0 (péremption 400 j appliquée seulement par le balayage, FdvPlan.perimer). (2) ref `RefObjet.placer_cimetiere` exige `ApiFinDeVie.passive || en_retard` et l'état "en_service" : or fin_de_vie note le cimetière AVANT d'appeler changer_etat_objet (ordre imposé), donc en_retard est déjà faux — un objet en retard non passivé, ou défaillant (hors service), ne peut pas être placé en cimetière. Proposition : placer_cimetiere accepte `FdvFin.etat == "cimetiere"` (nouveau getter `ApiFinDeVie.etat(objet)`) et l'état "hors_service".
- **Défauts de conception trouvés** : get_status de cette édition rend l'état initial pour un id inconnu (pas `()`) : sans lecture d'un slot d'existence, un id inconnu pouvait être pris en revue (corrigé par les reads ci-dessus). Fragmentation : FdvPlan.clore n'est pas appelé (la règle n'admet que rentre/cimetiere) → le plan reste "approuve" et sera rendu caduc par le balayage (manquement à tort) : point ouvert. Objet non attribué (État "") : ORBITE consent au titre de l'État.
- **Limites de Soma rencontrées** : typage nominal strict (Duree + Horodatage, String → Etat/Role/Org à convertir) ; `AutreCell.CONSTANTE` → délais par régime et grâce de 90 j recopiés en littéraux (FdvFin, FdvCouverture) ; doublures sans mémoire et bornées au cadre de la façade (pas d'appel à ApiAcces depuis une doublure ApiRef) ; List slot sans retrait par valeur (suppression par index, files tournantes dans les balayages).
