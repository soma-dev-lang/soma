# manoeuvres — notes d'architecture
**Découpage** (12 cells, 9 machines, 8 fichiers, ≈ 980 lignes) : `ManPlan` (cycle du plan = états de la façade), `ManCriblage`
(criblage obligatoire, clé plan), `ManEvitement` (verrou d'évitement, clé rapprochement ou "P:"+plan), `ManFenetre` et
`ManVerification` (clé plan), `ManAnomalie` (clé plan), `ManNonAnnoncee` (clé constat), `ManAvis` ("plan|org|sujet"),
`ManProgramme` (maintien à poste), sans machine : `ManVeille` (temps), `ManDossier` (preuve), `ManIndicateurs`.
**Propriété 1** : `crible -> approuve when ManEvitement.engage via verrou_de` ; le verrou ne se prend que libre (Conflit
sinon), revient à libre seulement si le titulaire n'a pas poussé (annulé, échec), se solde dès qu'une manœuvre est faite.
La déclaration et l'approbation exigent `ApiCoordination.arretee` et `designe(r, "systeme") == objet`. `reexaminer` (veille)
annule un plan approuvé dont la désignation a changé ⇒ le verrou est libéré AVANT qu'un autre plan puisse l'engager.
**Propriété 4** : `declare -> crible when ManCriblage.sur` (même id, ✓) ; approbation refaite à `h` : criblage < 8 h,
éphéméride fraîche ET identique au dépôt criblé, opérateur actif ; recriblage avant fenêtre si criblage > 8 h.
**Propriétés 2/3** : peut_manoeuvrer + pas en anomalie à la déclaration, à l'approbation et au réexamen ; habité ⇒
l'autre objet ne peut pas manœuvrer. **5** : aucune attente sans échéance (fenêtre, vérification, explication,
contradictoire, accusé) ; ManVeille les fait échoir ; avis sans accusé sur niveau 3 ⇒ ApiEscalade. **6** : `ManFondement`
écrit une fois à l'approbation ; avis et dossiers filtrés par ApiDonnees.peut_recevoir et tracés.
Autres règles posées (même id, ✓) : `approuve -> execute when ManFenetre.ouverte`, `execute -> verifie when ManVerification.conforme`, `execute -> anomalie when ManVerification.ecart || non_observee` (même id).
**Règles entre sous-systèmes proposées** :
- Cible = ManPlan.approuve ; exige = coordination:arretee (while) ; lien = `ManPlan.verrou_de` (= rapprochement ; à
  restreindre aux évitements : les verrous "P:" n'ont pas de coordination — sinon ajouter un slot `rapprochement_de`)
- Cible = ManEvitement.engage ; exige = coordination:arretee (while) ; lien = id (verrou = rapprochement)
- Cible = coordination:arretee → rouverte ; exige = manoeuvres:ManEvitement.libre|solde ; lien = r (pas de rouverture
  pendant qu'un plan approuvé tient le verrou sans être annulé)
- Cible = conjonctions:resolu (motif manœuvre) ; exige = ManPlan.verifie ; lien = ManPlan.plan_de(r)
**⚠ de verify** (aucun ✗, VERIFY OK skeleton) : invariants « runtime-checked » = slots écrits par des trous ;
`ensures` des signaux `-> ManEtatPlan` (String nominal) runtime-checked comme les Api* ; 1 obligation ouverte : `verrou_de`
écrit une fois par `declarer` (à prouver avec le corps). check : `len(slot)` voulu (borne de chaque liste par clé).
**Points ouverts** : manœuvre exécutée solde le verrou — un risque résiduel après écart exige un nouveau rapprochement
(TCA différent) ; fenêtre fixée par propulsion (pas de durée dans `declarer`) ; une poussée observée jamais confirmée
reste une anomalie, pas une exécution.
## Phase 1b (contrat v2)
Façade : 13/13 signaux branchés (facade.txt). Horloge : aucun `every`/`now` ; ManVeille.balayer ← ApiManoeuvres.balayer,
qui absorbe aussi la clôture des verrous (ApiConjonctions.clos_entre ; `clore_verrous` supprimé).
Trous fermés : plans_de / plan_couvrant (slot `tous_de` : une poussée dans une fenêtre déjà consommée reste « annoncée ») ;
approbation : designe(r, "systeme") == objet, depot_valide + avec_manoeuvres, désorbitation d'un objet à risque ⇒
ApiReentrees.controlee_approuvee ; transfert vérifié ⇒ ApiRef.changer_regime ; ManNonAnnoncee réécrit (manquement ouvert
au constat chez conformite, suite technique : régularisation par dépôt intégrant la poussée, sinon anomalie d'objet).
Réentrance : `retirer` (intégrateur) gardé, cause designation_retiree, n'appelle pas coordination. Tout appel à
ApiCoordination.rouvrir est précédé de la transition du plan et gardé par arretee (+ designe == objet) : pas de double
rouverture (ref / evenements rouvrent déjà). constater_non_annoncee n'appelle plus observation (son appelant).
en_anomalie retiré partout (inclus dans peut_manoeuvrer). plan_de rend aussi un plan verifie/anomalie (conjonctions le lit).
## Phase 1b — demandes
- coordination : n'appeler ApiManoeuvres.retirer que si ApiManoeuvres.etat ∈ {declare, crible, approuve}.

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
- **Signaux** : 84/84 remplis (12 cells + façade ApiManoeuvres), 9 fichiers `sys/manoeuvres/chair/*.cell`, 1 511 lignes (11 helpers privés). `banc_manoeuvres.cell` : check sans erreur.
- **Tests** : `test_manoeuvres.cell`, 15 `cell test`, 231 assertions, **toutes vertes**. Couvrent les 9 machines (plan, criblage, verrou, fenêtre, vérification, anomalie, non annoncée, avis, programme), les refus (habilitation, portée, quatre yeux, état, Perime, Delai, Conflit, Confidentiel…) et le scénario évitement complet (désignation arrêtée → déclaration → criblage → approbation + verrou + avis → exécution → vérification → résolu ; second plan refusé Conflit). Doublures : `doublures/manoeuvres.cell` (14 façades, sans état, monde lu dans les ids).
- **Cadres amendés** (`skeleton --amend`, check app OK) : ManFenetre.consommer/echoir/liberer `reads objet_de` (retirer la fenêtre de `par_objet` sans parcourir tous les objets) ; ManPlan.annuler `calls ApiCoordination.designe` (le commentaire exige designe == objet avant rouvrir) ; ManAnomalie.etat `reads nature_de` (rend "" si le plan n'a pas d'anomalie — sinon tout plan lisait "ouverte" dans le dossier).
- **Choix** : un contrôle d'approbation en échec ⇒ `rejeter` (cause gardée, pas de levée) ; verrou tenu par un autre plan ⇒ Conflit levé, donc ApiManoeuvres.declarer est refusé en bloc. Appels sortants non essentiels (invalidation, suivi renforcé, résolution, changement de régime, escalade, avis au propriétaire) en `try` + journal.
- **Demandes à l'intégrateur** : (1) ApiManoeuvres.annuler n'a pas de cause : la façade tente "proprietaire" puis, sur Habilitation, "orbite" — ajouter `cause: String` à la façade serait plus net.
- **Défauts trouvés** : ManNonAnnoncee.constater ne teste la couverture qu'à deux instants (`depuis` et `h`) faute d'un `plan_couvrant` sur intervalle — une fenêtre strictement intérieure échappe ; plan_couvrant compte aussi un plan en anomalie sans poussée (echec_execution) ; ManAnomalie.lever ne distingue pas Inconnu d'EtatInvalide (pas de `reads nature_de`) ; la veille compte un verrou déjà soldé comme changement si clos_entre le rend deux fois.
- **Trous** : aucun.
- **Soma** : types nominaux stricts (Horodatage + Duree refusé : `to_int`), constantes d'une autre cell recopiées (21 600 000, 2 000 — piège 3), retrait d'un List slot par `index_of` + `delete(i)`, pas de `take` (→ `slice`), appels nus homonymes (rejeter, annuler, solder) seulement avertis.
## Règles de barre (COLREG, 15/11/2026)
Règle 8 à l'approbation d'un évitement : pc résiduelle (`ApiConjonctions.pc_apres_plan`) < PC_FRANC (1 000 milliardièmes =
1/1 000 000), sinon rejet "manoeuvre_timide" ; approbation au plus tard à TCA − préavis, sinon "hors_delai" ; poussée incluse
(avec_manoeuvres, déjà exigé). Règle 17a : objet privilégié d'un rapprochement ouvert au TCA non échu ⇒ déclaration refusée
Conflit (tout motif), approbation rejetée "privilegie", plan hors évitement approuvé avant la pose annulé au réexamen (cause
"privilegie"). Règle 34 : l'avis « approbation » d'un évitement va au propriétaire du privilégié (+ État), texte d'intention
(heure, Δv, échéance d'accusé), journalisé. Seul le sujet "approbation" relit la coordination (retrait sans réentrance).
