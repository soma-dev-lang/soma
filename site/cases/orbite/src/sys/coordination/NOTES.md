# coordination — notes d'architecture
**Découpage** (8 cells, 7 machines) : `CoordDossier` (cycle de la coordination, clé r), `CoordVerrou` (verrou de désignation, clé r, réactif puis clos),
`CoordDecision` (décision + fondement figé, clé "DS-<r>-<n>"), `CoordSuivi` (exécution de la décision, même clé), `CoordReponse` (réponse attendue,
"<r>#<tour>|<org>"), `CoordArbitrage` (saisine d'ORBITE, "<r>#<tour>"), `CoordGrappe` (multi-objets, "GR-<pivot>-<n>"), `CoordEchange` (fil, sans machine).
**Propriété 1, par la structure** : la seule écriture d'une désignation est `CoordVerrou.poser`, qui prend l'arête `libre -> tenu` d'une instance par rapprochement :
un second `poser` échoue tant que le verrou est tenu. Une autre désignation exige `lever` (tenu -> libre), qui fait passer la décision en vigueur à `levee` et son
suivi à `caduc`. Règles dans les cadres : `CoordDossier accord|arbitrage -> arretee when CoordVerrou.tenu` (même id) ; `CoordVerrou libre -> tenu when
CoordDossier.accord || CoordDossier.arbitrage` ; `CoordDossier liberee -> rouverte when CoordVerrou.libre` ; `CoordDecision prise -> en_vigueur when
CoordVerrou.tenu`, `-> levee when .libre`, `-> soldee when .clos` (via rapprochement_de) ; `CoordSuivi attente_plan -> plan_depose when CoordDecision.en_vigueur`
(même id) ; `CoordReponse attendue -> acceptee|refusee when CoordDossier.proposee || desaccord via dossier_de` ; `CoordArbitrage * -> rendue when
CoordDossier.arretee via dossier_de`. Rouvrir annule le plan non exécuté de l'ancien désigné AVANT de lever le verrou ; refusé si le suivi est `execute` (jamais deux
évitements actifs). **À ajouter au soma.toml (phase 2, intra)** : `[verify.before."CoordDossier.arretee"] while = ["CoordVerrou.tenu"]`.
**Propriétés 2-3** : contrôlées à la pose (peut_manoeuvrer, en_anomalie, habité seulement si l'autre ne peut pas, opérateur actif, préavis ≤ TCA − h) et en continu
(`reverifier` au balayage ⇒ rouverture). **5** : échéance = min(délai contractuel au niveau, limite d'action − 2 h), jamais reculée (`recaler`) ; relance à mi-délai
; silence ⇒ muette + escalade + manquement + arbitrage ; suspendu ⇒ muette d'office ; l'arbitrage d'ORBITE est lui-même surveillé (hors délai ⇒ incident g3, escalade
échelon 4, arbitrage seul en accès d'urgence) ; désigné sans plan à la limite ⇒ défaillant, rouverture. **6** : fondement figé par décision (pc, TCA, niveau, règles,
dépôts et âges, réponses), jamais réécrit, purge après 10 ans hors gel, réserves des parties ; lectures filtrées par ApiDonnees et tracées.
**Règles entre sous-systèmes proposées** :
- Cible = CoordDossier.ouverte ; exige = conjonctions:alerte ; lien = r (même id)
- Cible = conjonctions:en_coordination ; exige = coordination:CoordDossier.ouverte (non close) ; lien = r
- Cible = CoordDossier.close ; exige = conjonctions:resolu || passe || collision ; lien = r
- Cible = manoeuvres:approuve (motif evitement) ; exige = coordination:CoordVerrou.tenu ; lien = `r_de` du plan (à créer chez manoeuvres, écrit à la déclaration)
- Cible = manoeuvres:declare (evitement) ; exige = coordination:CoordDecision.en_vigueur ; lien = `decision_de` du plan (à créer : CoordVerrou.decision(r) lu à la
déclaration)
- Cible = CoordSuivi.verifie ; exige = manoeuvres:verifie ; lien = CoordSuivi.plan_de
- Cible = responsabilite:purge ; exige = coordination:CoordDecision.purgee ou jamais prise ; lien = r
**Points ouverts / ⚠** : ⚠ de verify = obligations `via` (écriture unique de rapprochement_de, dossier_de) et `ensures` des types String
nominaux. Un rapprochement dont les deux objets sont pivots ne rejoint qu'une grappe (celle de a) ; grappe formée ⇒ arrêts suspendus jusqu'à
`regler`/`arbitrer` (le balayage tente `regler`). Ordre d'écriture des invariants `?? 0` : limite_de, echeance_de, ouvert_le ; `attendre` sème repondu_le.
## Phase 1b
- Façade v2 : 13/13 signaux branchés (facade.txt) ; nouveaux internes CoordVerrou.decidee, CoordDecision.resume, CoordReponse.muets / en_attente_de ;
`balayer` → CoordDossier.balayer (seul point d'entrée du temps ; aucun every/after/now dans coordination).
- Trous fermés : `designe(r, par)` filtré (ORBITE, "systeme", ApiDonnees.peut_recevoir) ; silence → arbitrage après ApiEscalade.reprise (filet :
limite − 2 h) ; crise → arbitrage direct (ApiOperations.arbitrage_centralise) à l'ouverture et à la rouverture ; jamais proposé/posé un objet
refusé par ApiRef.peut_manoeuvrer (anomalie/passivation incluses, ApiEvenements.en_anomalie retiré en doublon) ; fondement : distance,
ApiPriorites.decision_de, ApiEphemerides.fondement(objet, h) ; recalage sur ApiConjonctions.mis_a_jour ; suivi vérifie objet_de/motif_de du plan ;
arbitrage seul d'urgence via ApiAcces.en_urgence_pour(r) ; rouvrir retire le plan par ApiManoeuvres.retirer (pas de rappel : aucun cycle).
- Demandes : `ApiCoordination.designe(h, r, par)` — sans `h`, la diffusion est jugée à l'instant de la pose (pose_le), pas à la lecture.
`ApiManoeuvres.plan_de(r)` ne distingue pas un plan retiré d'un plan annulé par l'opérateur : ajouter `ApiManoeuvres.retire(plan) -> Bool`.
## Phase 2 (coordination + priorites)
**Règles écrites : 10, forme arête, aucune en toml.**
1. CoordDossier ouverte -> proposee | ouverte -> arbitrage ⇐ ConjRapprochement.alerte (même r) — ✓
2. CoordDossier * -> close ⇐ ConjRapprochement.resolu || passe || collision (même r) — ✓
3. CoordDecision prise -> designee ⇐ RefObjet.en_service via objet_de (désignation arrêtée ⇐ objet en service) — ✓, lien écrit par le trou prendre
4. CoordDossier arretee -> liberee ⇐ ManEvitement.libre || solde (même r ; rouverture ⇐ aucun évitement approuvé) — ✓
5. CoordSuivi execute -> verifie ⇐ ManPlan.verifie via plan_de — ✓, lien écrit par le trou suivre
6. CoordDecision levee|soldee -> purgee ⇐ RespPurges.autorisee via purge_de — ✓, lien écrit par le trou purger
7. PrioDerogation en_instruction -> recevable ⇐ OpMembres.actif via op_de — ✓ (trou demander)
8. PrioDerogation recevable -> eligible ⇐ RefObjet.en_service via objet_de — ✓ (trou demander)
9. PrioEssaim declare -> reconnu ⇐ OpMembres.actif via op_de — ✓ (trou declarer)
10. PrioDesignation archivee -> purgee ⇐ RespPurges.autorisee via purge_de — ✓ (trou purger)
**Écartées** : arrêtée ⇐ PrioDesignation.calculee|confirmee (un PD par tour, donc lien non unique ; l'arbitrage peut s'écarter des règles ; une contestation ne suspend rien) ; arrêtée ⇐ EphSuivi.a_jour (la fraîcheur est exigée à l'approbation du plan, propriété 4 ; l'exiger à la désignation bloquerait l'arbitrage) ; arrêtée ⇐ non FdvPassivation / non ObsDetections.confirmee (négations, déjà couvertes par peut_manoeuvrer) ; arbitrage ⇐ reprise d'escalade / message passerelle parti (le filet limite − 2 h est voulu, propriété 5, et le lien r → dossier ou message n'est pas unique, un par partie) ; close ⇐ aucun dossier d'escalade actif (négation) ; arrêtée ⇐ trace journalisée (pas une machine) ; PrioVersion.en_vigueur ⇐ hors gel (OpsGel n'a pas d'état global positif, tenu à l'exécution par ApiOperations.changement_autorise).
**Verify** (2 passes, dernière : VERIFY OK, 1031 ✓) : aucun ✗ ni « not explored » sur mes règles. Le seul ⚠ restant (EscVeille.couverte) appartient à escalade. Première passe : la règle ManEvitement posée sur CoordVerrou tenu -> libre visait un état initial (obligation impossible) : déplacée sur CoordDossier (règle 4).
**Structure** : états CoordDossier.liberee, CoordDecision.designee et abstention (décision « ne pas manœuvrer », levée et soldée comme en_vigueur), PrioDerogation.recevable et eligible. Slots CoordDecision.objet_de et purge_de, PrioDesignation.purge_de. Les deux `purger` prennent un paramètre `purge`. PrioDesignation.purger devient un geste « conformite » : l'échéancier signale la purge due sans l'exécuter (il n'a pas l'id de purge). Ajouts aux calls : ApiRef.etat_objet (CoordDecision.prendre, PrioDerogation.octroyer), ApiResponsabilite.purge_autorisee (CoordDecision.purger). cycles.py : aucune composante, profondeur 23.
## Phase 3 (corps)
- **Signaux** : 84/84 remplis (13 façade ApiCoordination + 71 internes sur 8 cells), 1 460 lignes de corps (`sys/coordination/chair/*.cell`), aucun trou laissé.
- **Tests** : `test_coordination.cell`, 12 tests / 222 assertions, tous verts (`$S test test_coordination.cell`) : scénario accord → verrou → plan → exécution → vérification → clôture ; désaccord → 2ᵉ refus → arbitrage 4 yeux → réserve → rouverture → nouvel accord ; refus (habilitation, portée, quatre yeux, motif, Incoherent, Delai, Existe, Conflit, NonEligible, Suspendu, Confidentiel, Ferme) ; silence (relance 1/2-3/4-7/8, muettes, reprise, filet limite − 2 h, retards, bilan) ; désigné défaillant → rouverture tardive → saisine hors délai → arbitrage seul en accès d'urgence ; reverifier ; purge 10 ans + gel ; grappe (formation, accord sans grappe → arbitrage, regler, compatible, dissolution) ; recalage. Doublures sans état (`doublures/coordination.cell`, 13 façades) : le monde est lu dans les ids (voir l'en-tête), le reste par `mock`.
- **Cadres amendés** (`--amend`, check app OK) : CoordDossier.arreter / constater_silence / arbitrer / recaler `reads tour_de` (CoordArbitrage.saisir exige le tour) ; CoordDossier.demander_rouverture `calls CoordEchange.noter` (renvoyer le nombre d'échanges du fil) ; CoordSuivi.rendre_caduc `reads rapprochement_de` (distinguer Inconnu d'un suivi en attente_plan sans transition).
- **Demandes à l'intégrateur** : (1) `ApiCoordination.designe(h, r, par)` (rappel 1b). (2) ajouter `ApiRef.orbite` aux calls de CoordArbitrage.prendre_en_charge, CoordGrappe.regler, CoordDossier.arbitrer/rouvrir/clore : ils notent au fil au nom d'ORBITE avec le littéral "ORG-ORBITE" (valeur contractuelle de ApiRef.orbite). (3) `ApiManoeuvres.retire(plan) -> Bool` (rappel 1b).
- **Défauts de conception** : `CoordDecision.consulter` promet le fondement « avec ses réserves » mais CoordFondement n'a pas de champ réserves (à ajouter : `reserves: List<String>`, rempli à la lecture). Les arêtes intra-coordination `when CoordVerrou.* / CoordDossier.* / CoordDecision.*` (Verrou.poser, Decision.prendre/lever/solder, Suivi.suivre/rendre_caduc, Reponse.enregistrer, Arbitrage.rendre, Dossier.arreter/rouvrir) n'ont pas de getter dans leurs calls : tenues par l'ordre des appels (transition du verrou avant prendre/lever/solder, etc.), à confirmer par verify. `arretee -> liberee when ManEvitement.libre||solde` est tenu par `ApiManoeuvres.etat(plan) != "approuve"` après retrait (pas de getter ManEvitement). CoordReponse.relancer passe par CoordEchange.notifier, qui écrit à toutes les parties (pas de notification ciblée). contester n'accepte qu'une décision de mode "arbitrage" (commentaire). Escalades/manquements appelés en `try` dans attendre / constater_muettes / suivi : une escalade refusée ne bloque jamais la coordination (propriété 5).
- **Limites de Soma** : `transition(id, variable, "x")` refusé hors `* -> x` (sources littérales en if/else) ; `pow` rend un Float (idiv refuse) ; une locale nommée comme le signal d'une autre cell (`actif`) est lue comme un appel à EvtNuages.actif ; mocks à un coup (files pour les balayages) ; types nominaux partout (`Org(p)`, `Objet(o)`, `to_int(h)`).
