# conformite — notes d'architecture

## Choix de conception
- Chaîne : `ConfManquement` (constat → contradictoire → instruction | forclos → qualifie → clos | conteste → clos | annule ; classe) → `ConfNotation` (points glissants 365 j, g1 5 / g2 15 / g3 40) → `ConfMesure` (avertissement, surveillance renforcée, demande de suspension) → `ConfDemande` (État de tutelle). `ConfRemediation` (plan après qualification g ≥ 2), `ConfVeille` (registre + balayage des façades), `ConfRapport` (annuel par État), `ConfIndicateur` (public trimestriel, k-anonymat K_MIN = 3).
- Même id à travers les machines : contradictoire et remédiation = id du manquement ; demande de suspension = id de la mesure. Règles internes sans lien : `ConfManquement contradictoire -> instruction when ConfContradictoire.repondu`, `contradictoire -> forclos when ConfContradictoire.forclos`, `ConfDemande preparee -> transmise when ConfMesure.decidee`. Avec lien (`fondement_de`, écrit une fois par `ConfMesure.proposer`) : `ConfMesure proposee -> decidee when ConfManquement.qualifie || ConfManquement.clos`, `en_vigueur -> levee when ConfRemediation.realisee`.
- Silence (propriété 5) : opérateur muet ⇒ forclusion à l'échéance (48 h en g3, sans prorogation) et la procédure continue ; État muet ⇒ `ConfDemande sans_reponse`, direction d'ORBITE avertie, cité au rapport ; rapport non accusé ⇒ `sans_accuse`.
- Traçabilité / diffusion (propriété 6) : chaque manquement porte sa `reference` (rapprochement, plan, dépôt…) ; `fiche`, `ConfRapport.lire` refusent `Confidentiel` hors conformité/direction/auditeur, personnel de l'opérateur, point focal/autorité de son État ; lectures journalisées. Les indicateurs publics ne publient que des agrégats.
- ORBITE ne suspend pas seule : demande à l'État, puis `ConfDemande.executer` → `ApiOperateurs.suspendre` (par "conformite", valideur "direction").
- Façade : `ApiConformite.constater` reçoit `nature: String` ; hors `ConfNature` ⇒ "autre", chaîne d'origine gardée dans `detail` (pas de refus HorsBornes possible : non listé dans la façade). `MotifRequis` = référence vide. Dédoublonnage sur "op|nature|reference" (id existant renvoyé).
- `sous_surveillance(op)` n'a pas d'horodatage : elle lit la dernière note calculée (à la qualification, au recours, au relevé quotidien `ConfNotation.relever`), plus g3 non terminaux, plus mesure "surveillance_renforcee" en vigueur. Les points sortent de la fenêtre au relevé.
- Une proposition automatique de mesure ne refuse jamais pour doublon (renvoie l'existante) : elle ferait échouer la qualification qui la déclenche.

## Règles entre sous-systèmes proposées
- `Cible = ConfDemande.executee ; exige = operateurs:opérateur "suspendu" ; lien = ConfDemande.op_de` (écrit une fois par `preparer`).
- `Cible = ConfDemande.transmise ; exige = ref:État participant ; lien = ConfDemande.etat_de` (écrit une fois par `preparer`).
- `Cible = ConfRapport.transmis ; exige = ref:État participant ; lien = ConfRapport.etat_de` (écrit une fois par `ouvrir`).
- `Cible = ConfManquement.forclos ; exige = passerelle:message de notification acquitté OU en échec définitif (escaladé) ; lien = ConfContradictoire.message_de` — on ne forclôt pas un opérateur jamais prévenu, sans que son silence bloque.
- Sens inverse (chez les autres) : `operateurs` suspendu par "conformite" ⇐ `ConfDemande.acceptee` ; `operateurs.retablir` d'un opérateur suspendu sur demande ⇐ `ConfMesure.levee || echue` ; `escalade` échelon ≥ 3, `coordination` silence passé l'échéance, `manoeuvres` exécution non conforme, `fin_de_vie` retard, `lancements` préavis manquant, `donnees` trace refusée ⇒ appeler `ApiConformite.constater` (la veille rattrape une partie seulement).

## Points ouverts
- verify : 0 ✗, VERIFY OK (skeleton). ⚠ : invariants de taille « runtime-checked » sur slots écrits par des trous (à tenir par `require len(...) <` dans les corps) ; 3 obligations ouvertes « `fondement_de` écrit une fois » (corps de `proposer`) ; `ConfMesure.nature` ensures sur type String runtime-checked (comme les `Api*` à résultat énuméré).
- Conservation/purge des manquements : non traitée ici (responsabilite) ; une purge devrait exiger `ApiResponsabilite.sous_gel(reference) == false`.

## Phase 1b — contrat v2
- Façade : 5/5 signaux branchés (facade.txt) ; `constater_etat` → `ConfManquementEtat` (nouvelle machine dans manquements.cell : constate → notifie → repondu | sans_reponse → clos ; classe ; pas de note ni de mesure, cité au rapport annuel) ; `balayer` → `ConfRonde.balayer` (veille.cell), seul lecteur du temps : échéances, inscription des actifs, balayage des opérateurs, silences, relevés de notes, rapports annuels. Aucun `every`/`now` (il n'y en avait pas).
- Trous fermés : opérateurs jamais constatés (`ApiOperateurs.actifs` → `ConfVeille.suivre`) ; rapports manquants (`ApiRef.etats_participants` → `ConfRapport.ouvrir`) ; forclusion d'un opérateur encore en cours de notification (`ApiPasserelle.echoue` → `ConfContradictoire.notification`, refus Insuffisant ; échec ⇒ "astreinte_absente") ; silences de coordination (`ApiCoordination.muets` sur `ApiConjonctions.urgents` → "reponse_tardive", `ConfVeille.balayer_silences`) ; États silencieux (demande ou rapport sans réponse ⇒ manquement d'État).
- `ApiPasserelle.notifier` : r = la référence si "CJ-…", sinon "".
- Demandes : `ApiEscalade` ne liste toujours pas les escalades graves d'un opérateur (référence de "refus_escalade" = compteur daté du jour) — ajouter `ApiEscalade.graves_de(op, h) -> List<String>` (rapprochements concernés).

## Phase 2 — règles entre sous-systèmes (escalade + conformite)
- Écrites : 8 règles (11 arêtes + 1 table toml), toutes dans le cadre du signal propriétaire :
  1. EscDossiers.reprise ⇐ ConjRapprochement.alerte || en_coordination via rapprochement_de (arête, ×5 : e1..e4, accusee ; reprendre)
  2. EscDiplomatie.transmise ⇐ RefEtat.participant via etat_vise (arête validee -> transmise ; transmettre)
  3. ConfDemande.executee ⇐ OpMembres.suspendu || OpMembres.exclu via op_de (arête acceptee -> executee ; executer)
  4. ConfDemande.transmise ⇐ while RefEtat.participant via ConfDemande.etat_de (TOML, sys/conformite/regles.toml : arête déjà prise par ConfMesure.decidee)
  5. ConfRapport.transmis ⇐ RefEtat.participant via etat_de (arête valide -> transmis ; transmettre)
  6. ConfContradictoire.forclos ⇐ PasMessages.acquitte via message_de (arêtes ouvert|proroge -> forclos ; forclore)
  7. ConfContradictoire.injoignable ⇐ PasMessages.escalade || PasMessages.signale via message_de (arêtes ouvert|proroge -> injoignable)
  8. ConfManquement.forclos ⇐ ConfContradictoire.forclos || injoignable (même id ; remplace la règle interne à un état)
- Structure : état terminal ConfContradictoire.injoignable (forclusion d'un opérateur que la passerelle n'a pu joindre ; évite 3 alternatives) ;
  arête EscDiplomatie validee -> abandonnee (classer, État devenu non participant) ; caduquer d'un dossier étendu au rapprochement retombé
  hors alerte sans coordination (sinon bloqué hors de la reprise). Aucun slot ajouté (liens existants, écrits une fois à la création).
- Getters ajoutés aux calls : ApiConjonctions.etat (EscDossiers.reprendre), ApiRef.etat_participant (EscDiplomatie/ConfDemande/ConfRapport.transmettre),
  ApiPasserelle.acquitte/echoue (ConfContradictoire.forclore). cycles.py : aucune composante, profondeur max 23.
- Verify (2 passes, copie scratch avec regles.toml fusionné dans soma.toml) : règles 1-8 sans ✗ ; seules « open obligation … written by the hole »
  (preparer/ouvrir : lien écrit une fois, jugé au corps). VERIFY OK à la 1re passe hors règle écartée ci-dessous.
- Écartées : EscVeille.couverte ⇐ CoordDossier.arretee (même id) — essayée sur ronde puis sur un signal dédié : « not proven, calls nest deeper
  than 12 (through EscVeille.armer) » puis ✗ (cône de la machine veille trop profond : ronde → EscDossiers.ouvrir → …) ; retirée, veille.cell
  revenu à l'état de phase 1. À reprendre en sortant armer/ouverture de dossiers de la ronde, ou en forme toml.
  Manquement « silence_escalade » ⇐ EscStatistiques.vigilance, et « manoeuvre_non_annoncee » ⇐ ObsDetections.signalee : une règle d'état ne
  peut viser une nature de manquement (ConfManquement.constate est l'état initial, commun à toutes les natures) — à tenir au corps.

## Phase 3 — corps (14/11/2026)
- Signaux remplis : 103 / 103 (8 cells internes + façade ApiConformite), 1 753 lignes de corps (sys/conformite/chair/*.cell), aucun trou laissé. `check banc_conformite.cell` : 0 erreur.
- Tests : test_conformite.cell, 19 tests / 338 assertions, tous verts (`$S test test_conformite.cell`, < 1 s). Cycles nominaux de toutes les machines (manquement, manquement d'État, contradictoire, mesure, demande, remédiation, rapport, publication), refus Habilitation / QuatreYeux / portée (opérateur, État) / EtatInvalide / Delai, scénario complet (2 g3 → note 20 → avertissement, surveillance, demande de suspension proposés → décidée → mise en vigueur → remédiation déposée, refusée, redéposée, acceptée, réalisée → levée → clôture), demande à l'État acceptée → exécutée, silence de l'État, ronde de bout en bout. Doublures : doublures/conformite.cell (sans état, tout se lit dans les ids).
- Cadres amendés (skeleton --amend, un seul coup, check app.cell OK, cycles.py sans composante) :
  ConfManquement.forclore `calls + constater` (astreinte_absente sur notification échouée) ; ConfManquement.classer, statuer_recours, clore `reads + nature_de, reference_de` (clé de dédoublonnage à effacer) ; ConfManquementEtat.constater_silence `reads + etat_de` (Inconnu) ; ConfDemande.executer `calls + ApiAcces.a_le_role, ApiAcces.quatre_yeux` (opérateur déjà suspendu : suspendre n'est pas appelé, l'habilitation n'était plus contrôlée) ; ConfRapport.valider, annuler `reads + etat_de` ; ConfRemediation.constater_realisation `reads + op_de` ; ConfIndicateur.calculer `calls + ConfVeille.operateurs_suivis, ApiRef.etats_participants` (aucune liste d'opérateurs ni d'États sinon) ; ConfIndicateur.publier, rejeter `reads + debut_de`.
- Demandes à l'intégrateur : aucune nouvelle sur les façades (reste ApiEscalade.graves_de(op, h) de la phase 1b).
- Défauts de conception trouvés (non corrigés, à amender) :
  1. Recours accueilli (annule) : la remédiation demandée à la qualification reste "demandee" ⇒ la ronde la déclarera défaillante et constatera "remediation_non_tenue" sur un manquement annulé ; une mesure en vigueur fondée sur lui n'est pas rappelée. Ajouter ConfRemediation `demandee|deposee|refusee|acceptee -> sans_objet` appelée par statuer_recours.
  2. Indicateurs : aucune source par nature ni de forclusions ⇒ "manquements_par_nature" publié par gravité, "taux_forclusion=non_disponible". Ajouter ConfManquement.compter_natures(de, a) et ConfContradictoire.compter(de, a) -> [repondus, forclos, injoignables].
  3. ConfManquement.en_souffrance : date d'instruction / forclusion non tenue ⇒ délai de qualification compté depuis le constat. ConfMesure.compter : date de décision non tenue ⇒ mesures entrées en vigueur seulement.
  4. ConfContradictoire.ouvrir ne reçoit pas la référence : le rapprochement "CJ-…" est le premier mot du résumé (convention posée par ConfManquement.notifier). ConfManquement.proroger ne lit pas `cycle` : EtatInvalide délégué à ConfContradictoire.proroger (non notifié ⇒ Inconnu).
  5. ConfRonde : étapes "actifs" et "notes" non bornées par MAX_PAR_ETAPE (pas de curseur persistant) ; ConfVeille.balayer compte aussi les constats dédoublonnés (pas de getter « nouveau »). ConfRapport.transmettre vers un État non participant refuse (EtatInvalide) sans annuler : l'annulation reste un geste de la direction.
- Limites de Soma rencontrées : typage nominal strict (Horodatage + Int refusé : `Horodatage(to_int(h) + d)` ; Gravite → Niveau : `Niveau(to_int(g))` ; String → Objet / Org / ConfNature à construire ; paramètres des helpers à typer nominalement) ; `e.value` des entrées de slot non typé (17 avertissements) ; une doublure ne peut pas appeler une autre façade hors de son cadre (ApiOperateurs.suspendre : rôles lus dans les noms).
