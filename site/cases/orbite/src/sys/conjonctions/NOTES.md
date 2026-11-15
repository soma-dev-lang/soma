# conjonctions — notes d'architecte

## Choix de conception
- 9 fichiers, 10 cells : `ConjCadence` + `ConjCampagne` (campagnes), `ConjRenforcement` (tempête/nuage), `ConjSeuils` (table de niveau versionnée, quatre yeux, défauts en constantes), `ConjRapprochement` (cycle de ApiConjonctions.etat), `ConjAnalyseTca` (après TCA), `ConjMessage` + `ConjAbonnement` (CDM/alertes), `ConjCriblagePlan`, `ConjContestation` ; types fermés dans `vocabulaire.cell`.
- Un rapprochement n'est jamais clos « passe » par le seul temps : `ConjAnalyseTca` (même id) réacquiert les objets ; passe/collision exigent son verdict (règle sur l'arête, prouvée ✓ sur le produit).
- Le catalogue ne « oublie » rien en silence : `ConjCampagne.declarer_exclus` (objets sans trajectoire exploitable ⇒ suivi renforcé ; deux exclusions de suite ⇒ conformité), `ConjRapprochement.demander_mesures` (objet mal connu au niveau ≥ 2), covariance « faible » ⇒ niveau ≥ 2 sous le seuil de distance.
- Propriété 4 : `ConjCriblagePlan.cribler` refuse Perime (régime non criblé depuis 8 h, éphéméride non fraîche, plan commencé, plan caduc) ; un plan « sur » devient caduc à tout nouveau rapprochement de niveau ≥ 2 sur l'objet (`rendre_caducs`). Instance = id du plan de MANŒUVRES (une fois ; caduc ⇒ nouveau plan).
- Propriété 5 : alerte niveau 3 avec échéance (ApiOperateurs.delai_reponse, bornée par le TCA) ; sans accusé ⇒ `sans_reponse -> escalade` (ApiEscalade) ; sans astreinte ⇒ escalade immédiate.
- Propriété 6 : chaque mise à jour garde ses fondements (dépôts des deux objets, version des seuils, facteur météo, campagne) ; `fondements` ne les livre qu'après ApiDonnees.peut_recevoir sur les DEUX objets, et trace. Les messages contrôlent la diffusion de l'objet de l'AUTRE partie.
- Cadence ≤ 6 h par invariant : le catalogue ne peut pas devenir « périmé » (8 h) par simple réglage.
- Règles internes sur l'arête : `ConjRapprochement * -> passe when ConjAnalyseTca.sans_suite || infirmee`, `* -> collision when ConjAnalyseTca.confirmee` (même id, ✓) ; `ConjCriblagePlan demande -> sur / a_risque when ConjCampagne.terminee via campagne_de` ; `ConjRenforcement fin_demandee -> leve when ConjCampagne.terminee via campagne_de` ; `ConjContestation examinee -> acceptee when ConjCampagne.terminee via campagne_de`.

## Règles entre sous-systèmes proposées
- Cible = ConjRapprochement.en_coordination ; exige = coordination:CoordCoordination ouverte (ou au-delà) ; lien = id du rapprochement
- Cible = ConjRapprochement.resolu (depuis en_coordination) ; exige = coordination:arretee ou close ; lien = id du rapprochement
- Cible = ConjCriblagePlan.sur ; exige = manoeuvres:plan "declare" ; lien = id du plan
- Inverse, côté manoeuvres : ManPlan.approuve exige (while) conjonctions:ConjCriblagePlan.sur ; lien = id du plan (propriété 4)
- Inverse, côté coordination : ouverture exige (while) conjonctions:ConjRapprochement.alerte ; lien = id du rapprochement
- Cible = ConjAnalyseTca.confirmee ; exige = evenements:collision déclarée ; lien = ConjAnalyseTca.evenement_de (écrit une fois par confirmer)
- Cible = ConjMessage.escalade ; exige = escalade:escalade ouverte pour (r, op) ; lien = ConjMessage.rapprochement_de

## Points ouverts
- Appels croisés : signaler → engager → ApiCoordination.ouvrir → ApiConjonctions.etat ; cribler_plan → ApiManoeuvres.etat ; constater_collision ← ApiEvenements.declarer (réentrance à trancher).
- La façade n'a pas de `par` pour cribler_plan : identité "systeme" ; l'habilitation reste chez MANŒUVRES.
- Deux rapprochements peuvent décrire le même évènement si le TCA dérive de plus de 10 min entre deux criblages (pas de fusion).

## Avertissements expliqués
- check : `len(slot)` en invariant borne volontairement chaque liste (pas le nombre d'entrées) ; invariants entre slots : ordre d'écriture indiqué en commentaire.
- verify : 0 ✗ ; ⚠ = trous (frames), contrats de types nominaux runtime-checked, 4 « open obligation » d'écriture unique des liens `campagne_de` (jugées avec les corps).

## Phase 1b — contrat v2
- Façade : 20/20 signaux branchés (facade.txt). Nouveaux internes : ConjRapprochement.historique / clos_entre / urgents(h, horizon, niveau_min) ; ConjFiche.mis_a_jour ; ConjCampagne.fraiche ; ConjExemption (accorder, avancer, couvre : couple plafonné au niveau 1, ApiFinDeVie.retrait_autorise, quatre yeux) ; ConjRenforcement.renforcer (appelé par meteo/evenements, idempotent : repousse la fin).
- Horloge : il n'y avait ni every ni now ; tous les « ticks » passent par ConjRonde.balayer (← ApiConjonctions.balayer) : campagnes, épisodes, exemptions, réévaluations, analyses après TCA, messages, contestations, abonnements.
- Collision : ConjAnalyseTca.confirmer n'appelle plus ApiEvenements.declarer ; c'est evenements qui rappelle constater_collision → confirmer (ouverte|suspecte -> confirmee, vérifie existe/genre/objets) → clore_apres_tca.
- Trous fermés : contestation rejetée si ApiEphemerides.depot_valide faux ; échec de livraison (ApiPasserelle.echoue) ⇒ sans_reponse ⇒ escalade ; criblage d'un plan exposé par criblage_plan (état `caduc` renommé `perime`) ; météo poussée (ApiMeteo.renforce) au lieu d'être sondée.
- Demandes : ApiObservation.suivi_depuis(objet, depuis) -> Bool (réacquisition POSTÉRIEURE au TCA ; aujourd'hui `suivi` peut valoir sur une orbite d'avant le TCA). ApiConjonctions.renforcer devrait lister Incoherent (fin ≤ h) dans ses refus.

## Phase 2 — règles entre sous-systèmes (conjonctions + meteo)
- Écrites (7) : arête sauf mention.
  1. ConjRapprochement.en_coordination ⇐ CoordDossier.proposee || arbitrage (même id r ; `ouverte` n'est que transitoire dans ApiCoordination.ouvrir) — engager.
  2. ConjRapprochement résolu par manœuvre ⇐ ManPlan.verifie via plan_de (nouveau slot, écrit une fois par resoudre) : nouvel état transitoire `evite` (en_coordination -> evite when … -> resolu) ; « manoeuvre_verifiee » seulement depuis en_coordination.
  3. ConjCriblagePlan.sur ⇐ ManPlan.declare (même id plan) : nouvel état transitoire `fonde` (demande -> fonde when ConjCampagne.terminee via campagne_de ; fonde -> sur when ManPlan.declare ; fonde -> a_risque). La forme toml (while) sortait ✗ (trou cribler sans la clause) : abandonnée.
  4. ConjAnalyseTca.confirmee ⇐ EvtEvenements.signale || confirme via evenement_de (2 arêtes, confirmer).
  5. ConjMessage.escalade ⇐ EscVeille.surveillee || couverte via rapprochement_de (escalader ; la veille par rapprochement est armée par EscDossiers.ouvrir).
  6. MetSource.active (declaree -> active, qualifier) ⇐ RefOrg.inscrite || active via fournisseur ; calls + ApiRef.organisation_existe.
  7. MetMesure.archivee ⇐ RespGels.libre via gel_de (nouveau slot "J:<jour>", écrit par recevoir) — TOML, sys/meteo/regles.toml (arête déjà prise par MetJournee.scellee).
- Écartées : resolu ⇐ coordination arretee|close (circulaire avec CoordDossier.close ⇐ conj resolu, et pc retombée doit résoudre avant l'arrêt) ; resolu ⇐ ReeCampagne.rentree (aucun motif « rentrée », lien ambigu entre deux objets) ; MetBareme.en_vigueur ⇐ changement autorisé (fait global ApiOperations.changement_autorise, aucune machine/lien par version) ; ConjRenforcement ⇐ MetRenforce non levé (épisode multi-causes tempête/nuage/manuel, pas de lien météo) ; MetSource retablir/reprendre_service non conditionnés (arêtes de retour).
- verify (2 passes, copie avec regles.toml fusionné) : 0 ✗ et 0 « not explored » sur mes règles ; ⚠ seulement « open obligation … written by the hole » (plan_de, evenement_de, rapprochement_de, fournisseur, gel_de). Seul ✗ global : EscVeille.couverte ⇐ CoordDossier.arretee (escalade, pas à moi). check 0 erreur ; cycles.py : aucune composante, profondeur max 23.
- Structure : états `evite` (ConjRapprochement), `fonde` (ConjCriblagePlan) ; slots ConjRapprochement.plan_de, MetMesure.gel_de ; refus EtatInvalide ajouté à cribler.

## Phase 3 — corps
- Signaux remplis : 101/101 (100 du contrat + noyau natif `ConjCampagne.criblage`), ~1 950 lignes de corps dans `sys/conjonctions/chair/` (9 fichiers) ; doublures `doublures/conjonctions.cell` (15 façades, sans mémoire, conventions en tête de fichier).
- Tests : `test_conjonctions.cell`, 24 cellules de test, 275 assertions, toutes vertes (cycles des 12 machines, refus Habilitation/QuatreYeux/portée/état, scénario complet criblage → alerte → coordination → plan sûr → manœuvre vérifiée → résolu/caducs/clôture). Noyau natif testé sur catalogue synthétique calculé à la main.
- Cadres corrigés (amendés aussitôt, `check app.cell` OK) : `ConjCampagne.criblage` ajouté (noyau [native], pur) ; `ConjCampagne.declarer_exclus` + `reads campagne` (refus EtatInvalide impossible sinon) ; `ConjRapprochement.demander_mesures` + `reads rapprochement` (refus Ferme) ; `ConjCriblagePlan.cribler` + `calls ConjRapprochement.ouverts_de, ConjRapprochement.fiche` ; `ConjRonde.balayer` + `calls ConjRenforcement.etat, ConjExemption.etat, ConjContestation.etat, ConjAbonnement.etat` (compter les changements d'état). cycles.py : aucun cycle.
- Défauts de conception : (1) AUCUN appelant de `ConjCampagne.terminer`/`declarer_exclus` hors du sous-système, et aucune façade ne livre de trajectoires (positions d'éphémérides) : les campagnes démarrées par la ronde échouent à 2 h ; le noyau natif est prêt mais rien ne l'alimente. (2) `cribler` ne peut donc pas cribler contre le catalogue : modèle documenté dans chair/plans.cell (poussée tangentielle, dérive 3·dv·Δt, recalcul des rapprochements OUVERTS de l'objet dans [debut, debut+7 j]) — il compte les risques que le plan garde, pas ceux qu'il créerait. (3) `en_cours_de` (≤ 12 clés « portée|régime ») : une seule campagne « objet » par régime ⇒ deux contestations simultanées dans un même régime : la seconde est refusée Existe ; un renforcement pendant une campagne de régime non close reste sans campagne liée (levée impossible avant un nouveau renforcer ou un abandon). (4) Règles sur l'arête sans getter dans le cadre : `ConjAnalyseTca.confirmer` (EvtEvenements.signale||confirme) n'a que `ApiEvenements.existe` ; `ConjMessage.escalader` (EscVeille) se fie à l'échelon ≥ 1. (5) `a_lancer` rend des régimes sans motif : la ronde planifie toujours « cadence » (le motif « renforce » ne sert qu'à la campagne ouverte par renforcer).
- Demandes à l'intégrateur : `ApiConjonctions.executer_campagne(h, campagne, catalogue: String, par) -> Int` (calls ConjCampagne.criblage, ConjRapprochement.signaler, ConjCampagne.terminer) ou équivalent chez OBSERVATION/EPHEMERIDES ; `ApiEvenements.etat(evenement) -> String` dans les calls de confirmer ; `ApiEscalade.veille(r) -> String` dans les calls de ConjMessage.escalader ; `ApiConjonctions.renforcer` : citer Incoherent (déjà dit en phase 1b).
- Trous laissés : aucun. Limites de Soma : types nominaux à convertir partout (Org(x), Horodatage(x)) — check les signale bien ; `to_int`/`Regime(...)` suffisent ; pas de mémoire possible dans une doublure de façade (doublures par conventions de noms).

## Règles de barre (COLREG, 15/11/2026)
- Géométrie : `ConjRapprochement.preciser_geometrie` (vitesse relative au TCA, m/s, 1..20 000, "systeme"/"analyste"), getters
  `vitesse_relative` (0 sans géométrie) et `en_rattrapage` (< SEUIL_RATTRAPAGE_MPS = 100 m/s) — façade ApiConjonctions.
- Règle 8 : `ConjCriblagePlan.cribler` mémorise la pc résiduelle de chaque rapprochement recalculé ("plan|r"),
  lue par `pc_apres` / `ApiConjonctions.pc_apres_plan`.
