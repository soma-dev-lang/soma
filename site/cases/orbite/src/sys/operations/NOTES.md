# operations — notes d'architecte

## Choix de conception
- 7 fichiers, 10 cells, 9 machines : `incidents` (OpsIncident, OpsPostMortem), `crise`, `degrade`, `secours` (OpsBascule),
  `changements` (OpsGel, OpsGelUrgence, OpsChangement), `continuite` (OpsTest), `ronde` (OpsRonde, sans machine).
- **Un id, trois machines** : incident, post-mortem et cellule de crise partagent l'id de l'incident. Règles sans slot :
  `OpsIncident resolu -> clos when OpsPostMortem.publie || dispense`, `OpsCrise active -> levee when OpsIncident.resolu`.
- **Slots `via`** (écrits une fois) : `OpsBascule.crise_de` (bascule réelle seulement sous crise active),
  `OpsChangement.incident_de` (urgence seulement si l'incident est en traitement ou contourné ; "" si normal).
- **Gel d'urgence = une instance par rapprochement** (id = Rapprochement, même id que conjonctions) : armé à niveau 3
  et TCA ≤ 6 h, levé seulement à la clôture du rapprochement ou TCA + 1 h (une baisse de niveau ne lève pas).
  `changement_autorise` = ¬gel ∧ ¬gel d'urgence (armé, ou urgents(h, 6 h, 3) ≠ []) ∧ ¬majeur ∧ ¬transition.
- **Fenêtre suspecte** (effet_depuis → resolu_a) : un incident d'effet "faussees" (criblage faux, alerte non partie)
  exige la revue des décisions de trafic prises dans la fenêtre (post-mortem) et un dossier de preuve chez
  responsabilite avant clôture (propriété 6).
- Mode dégradé par sous-système (OpsDegradation, réactive) avec procédures de repli typées, réexamen à 72 h, levée à
  quatre yeux ; la sonde `constater_couverture` dégrade l'observation sous 3 capteurs (trou « couverture » fermé).

## Règles entre sous-systèmes proposées
- Cible = OpsGelUrgence.leve ; exige = conjonctions:rapprochement resolu || passe || collision ; lien = même id (Rapprochement)
- Cible = OpsGelUrgence.arme ; exige = conjonctions:rapprochement alerte || en_coordination ; lien = même id
- Cible = manoeuvres:plan approuve ; exige = operations:OpsBascule ni en_cours ni retour_en_cours (pas de décision
  pendant une transition de site) ; lien = instance unique (OpsBascule.transition)
- Cible = OpsIncident.clos (effet "faussees") ; exige = responsabilite:dossier gelé ; lien = OpsIncident.dossier
- Cible = OpsCrise.active ; exige = acces:pilote actif ; lien = OpsCrise.pilote
- Cible = OpsChangement.en_deploiement ; exige = acces:demandeur actif ; lien = OpsChangement.demandeur

## Points ouverts / avertissements
- ⚠ « invariant between slots » (début/fin : absent = `?? 9e15`, « pas encore ») ; ⚠ verify : trous, bornes `.size`,
  `ensures` String, 2 open obligations (`crise_de`, `incident_de` écrits par des trous). Aucun ✗.

## Phase 1b (contrat v2)
- Façade : 6/6 signaux branchés (facade.txt) ; `arbitrage_centralise` → OpsCrise.arbitrage_centralise_en_vigueur,
  `balayer` → OpsRonde.balayer (nouvelle cell `ronde.cell` : gels, gels d'urgence, couverture, information
  périodique des États, relances dédoublonnées toutes les 30 min, purge). Aucun every/after/now (il n'y en avait pas).
- Trous fermés : gel d'urgence sur ApiConjonctions.urgents dans OpsGelUrgence.actif_a (plus de dépendance à
  l'armement) ; information de crise à TOUS les ApiRef.etats_participants (paramètre `etats` supprimé) ;
  gels programmés échus levés par la ronde (OpsGel.echus) ; notifier(h, dest, r, …) avec r ou "".
- Demande : le cadre de la façade ne porte pas de `calls` pour `arbitrage_centralise` et `balayer` — ajouter
  `calls OpsCrise.arbitrage_centralise_en_vigueur` et `calls OpsRonde.balayer` dans sys/operations/api.cell.

## Phase 2 — règles entre sous-systèmes (donnees + operations)
- Écrites : 4 (3 arête, 1 toml).
  (1) DonEmbargos.actif ⇐ RefObjet.catalogue || RefObjet.en_service via objet_de — arête demande -> actif (accorder ; + calls ApiRef.etat_objet ; refus EtatInvalide).
  (2) DonAdhesions.active ⇐ RefOrg.inscrite || RefOrg.active via org_de — toml (sys/donnees/regles.toml ; arête signee -> active déjà prise par DonAccords.en_vigueur, et suspendue -> active est une arête retour) ; contresigner et retablir + calls ApiRef.organisation_existe, refus EtatInvalide.
  (3) OpsGelUrgence.arme ⇐ ConjRapprochement.alerte || en_coordination, même id — arête veille -> arme (armer ; + calls ApiConjonctions.etat).
  (4) OpsChangement.en_deploiement ⇐ AccesPersonnes.active via demandeur — sur les deux arêtes approuve / approuve_urgence -> en_deploiement (deployer ; + calls ApiAcces.role_de ; refus EtatInvalide).
- Écartées : DonDemandes.accordee ⇐ objet catalogué (arête prise ; accès aux données d'un objet rentré/fragmenté légitime pour preuve et réclamation ; existence contrôlée par deposer) ;
  DonViolations.constatee ⇐ manquement constaté (le manquement est créé par qualifier lui-même : tautologique ; lien écrit à la qualification, pas à la création ; aucun getter d'état de manquement dans ApiConformite) ;
  OpsGelUrgence.leve ⇐ rapprochement resolu|passe|collision (3 alternatives, et contredit la sortie voulue « TCA + 1 h » : conjonctions en panne ne doit pas geler les changements indéfiniment) ;
  OpsIncident.clos ⇐ dossier gelé (conditionnelle à effet "faussees", arête prise, lien écrit par geler_preuves après création : reste au corps) ;
  OpsCrise.active ⇐ pilote actif (pilote = par d'activer, inconnu à la création ; ApiAcces.a_le_role exige déjà une personne active) ;
  purge des traces ⇐ RespPurges.autorisee (DonTraces sans machine ; corps via ApiResponsabilite.purge_autorisee).
- verify (1 passage) : VERIFY OK, aucun ✗ ni « not explored » ; (3) ✓ ; (1) et (4) « open obligation … written by the hole » (objet_de par demander, demandeur par soumettre) ; (2) toml non vérifiable avant fusion dans soma.toml.
- Structure : aucun état ni slot ajouté ; commentaires de slot (demandeur écrit une fois par soumettre) et fiches mises à jour ; retablir lit org_de. cycles.py : aucune composante, profondeur max 23.

## Phase 3 — corps
- Signaux remplis : 98/98 (92 de cells internes + 6 de façade ApiOperations), 4 helpers privés ; 1 362 lignes de corps (sys/operations/chair/*.cell, 8 fichiers).
- Tests : test_operations.cell, 19 cells de test, 329 assertions, toutes vertes (`$S test test_operations.cell`). Couvre les 9 machines de bout en bout, les refus (Habilitation, QuatreYeux, portée, EtatInvalide, Delai, Suspendu, HorsBornes, NonEligible, Echu, Insuffisant…), la ronde (gels, gels d'urgence, couverture, information des États) et un scénario métier complet (ScenarioCriblageFaux : incident majeur à effet « faussees » → crise → arbitrage centralisé + mode dégradé → correctif d'urgence → résolution → levée → post-mortem → preuves → clôture). Doublures : doublures/operations.cell (ApiAcces, ApiRef, ApiPasserelle, ApiDonnees, ApiConjonctions, ApiObservation, ApiResponsabilite).
- Cadres corrigés (amendés en une fois, `skeleton --amend`, `check app.cell` OK, cycles.py : aucune composante) :
  OpsIncident + `effet_de` et `vise_de` (nouveaux getters), qualifier + `reads declare_a` (délai de qualification) ;
  OpsPostMortem.soumettre + `calls OpsIncident.effet_de` (Insuffisant si « faussees » sans décision) ; publier + `calls ApiRef.org_de_etat` (points focaux) ;
  OpsCrise.activer/annuler/informer_etats/active_pour + `reads convoquee_a` (sans lui, un id jamais convoqué passait pour « convoquée » : état initial implicite) ;
  OpsDegradation.decreter + `calls OpsIncident.vise_de, OpsCrise.etat` ; OpsBascule.demander/autoriser + `calls OpsCrise.etat` (active_pour = convoquée OU active, la règle exige « active ») ;
  OpsGel.annuler + `reads debut`, OpsTest.annuler/etat + `reads scenario` (Inconnu, "inconnu") ; OpsChangement.soumettre + `calls OpsIncident.vise_de` ; OpsRonde.balayer + `calls OpsDegradation.est_degrade` (compter un décret de couverture une seule fois).
- Demandes à l'intégrateur : aucune (la demande de phase 1b sur api.cell est déjà satisfaite).
- Défauts de conception relevés, laissés au corps : « membre de la crise » (OpsBascule.demander) non contrôlable, aucun getter de membres ; OpsTest.demarrer ne peut pas prévenir les opérateurs d'exercice (pas de calls ApiPasserelle) — journalisé ; OpsDegradation.constater_couverture lie le mode dégradé à l'incident COUV-<h> qu'elle déclare (le commentaire de slot disait "") ; OpsCrise.arbitrage_centralise est une clé unique : deux crises simultanées partagent la mesure, la levée de l'une la ferme ; OpsChangement.constater_retour ne déclare pas l'incident d'échec (pas de calls) ; ApiOperations.degrade(ss) vrai pour tout ss pendant une bascule en transition (choix).
- Trous laissés : aucun.
- Limites de Soma : transition() exige des cibles littérales (cible calculée refusée par check : branches if écrites) ; types nominaux stricts dans les cells de test (Horodatage(…) dans les helpers, to_int dans la doublure) ; un `mock` scalaire n'est qu'une réponse, et un assert_fails placé avant l'appel qui devait la consommer a fait échouer le test (déplacé).
