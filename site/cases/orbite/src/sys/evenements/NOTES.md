# evenements — notes d'architecture

## Choix de conception
- 8 fichiers utiles, 8 cells, 7 machines : EvtEvenements (evenement), EvtNuages (nuage), EvtFragments (fragment),
  EvtAnomalies (anomalie), EvtNotifications (notification), EvtCollisions (collision), EvtRapports (rapport) ;
  EvtVeille sans machine (ronde à `h` monotone). abonnements.cell : vide, retiré en 1b (voir demandes).
- UN identifiant suit l'évènement : nuage, instruction de collision et rapport portent l'id "EV-<n>" ; règles internes
  sans `via` (nuage en_catalogage ⇐ évènement confirmé, abandonne ⇐ infirmé ; évènement clos ⇐ nuage dissipé ;
  collision instruite ⇐ confirmé…). Seule règle via : fragment rattache ⇐ nuage en catalogage (nuage_du_fragment).
- Criblage renforcé dès le signalement (ApiConjonctions.renforcer). Avert. : invariants entre slots, ⚠ runtime-checked.
- Propriété 2 : en_anomalie vrai dès la déclaration (lu par ApiRef.peut_manoeuvrer) ; levée sur preuve, quatre yeux.
- Propriété 6 : décision de coordination figée au signalement d'une collision (ApiCoordination.decision, plan, pc,
  TCA) ; verdict à quatre yeux ; "decision_ignoree" ⇒ manquement gravité 3 ; clôture impossible sans dossier ni
  instruction transmise ; historiques d'estimation et de versions du rapport.
- Notifications après ApiDonnees.peut_recevoir (sinon "retenue") ; silence d'un opérateur ⇒ manquement, jamais blocage.

## Règles entre sous-systèmes proposées
- Cible = EvtEvenements.confirme ; exige = ref:objets en cause catalogués, non rentrés ; lien = EvtEvenements.objets_de
- Cible = EvtEvenements.clos ; exige = responsabilite:dossier gelé (collision) ; lien = EvtEvenements.dossier_de
- Cible = EvtAnomalies.levee ; exige = observation:suivi < 72 h ET ephemerides:fraîche ; lien = EvtAnomalies.objet_de
- Cible = EvtNotifications.envoyee ; exige = donnees:peut_recevoir(destinataire, objet) ; lien = destinataire + objet_de

## Phase 1b
- Façade v2 : 10/10 signaux branchés (facade.txt) ; existe, objets, genre, instant, evenement_de sur EvtEvenements
  (slots renommés genre_de, instant_de, dernier_de) ; balayer → EvtVeille.ronde (-> Int). Aucun every/now.
- Trous fermés : declarer avec r ⇒ collision constatée (ApiConjonctions.constater_collision) et dossier gelé
  automatiquement (ApiResponsabilite.geler, "systeme") ; anomalie avec genre ; alerte de nuage et rapport à TOUS les
  États participants et, page par page (ApiRef.catalogue + proprietaire), aux propriétaires du régime (fin de
  l'opt-in) ; pistes vérifiées contre ApiObservation.pistes_non_correlees ; décision figée par ApiCoordination.decision ;
  ApiPasserelle.notifier avec r.
- Cycles : plus aucun appel à ApiCoordination.rouvrir/designe (rouvrir rappelait en_anomalie via priorites → ref) ;
  la réouverture est laissée au balayage de coordination. Aucun cycle de signaux. Restent des relectures de
  getters feuilles (sans écriture) imposées par le contrat : ApiResponsabilite.geler → ApiEvenements.existe/objets ;
  ApiRef.changer_etat_objet / declarer_propulsion → ApiCoordination.rouvrir → … → ApiEvenements.en_anomalie.

## Phase 1b — demandes
- app.cell : retirer `use sys::evenements::abonnements` (le fichier sera supprimé).
- coordination : ApiCoordination.balayer doit appeler CoordDossier.reverifier sur toute coordination arrêtée.
- responsabilite : RespDossiers.ouvrir sans `ApiEvenements.existe` quand par = "systeme" (l'appelant garantit l'id)
  supprimerait le cycle evenements ↔ responsabilite.

## Phase 2 — règles entre sous-systèmes (evenements + responsabilite, 14/11/2026)
Écrites (4) :
- EvtCollisions.transmise ⇐ RespGels.gele (même id évènement) — arête instruite -> transmise ; + calls ApiResponsabilite.sous_gel (refus Insuffisant). Remplace « EvtEvenements.clos ⇐ dossier gelé » : clore exige déjà une collision transmise.
- EvtAnomalies.levee ⇐ EphSuivi.a_jour via objet_de — arête en_suivi -> levee (getter déjà présent : ApiEphemerides.fraiche, refus Perime).
- RespFautes.designation_presumee ⇐ ConjRapprochement.passe || collision via rapprochement_de (arête) ET requires CoordDossier.arretee via rapprochement_de (regles.toml). Couvre « presumee ⇐ arretee » et « retenue ⇐ passe/collision » (états terminaux, donc encore vrais à la qualification). + calls ApiConjonctions.etat, ApiCoordination.etat.
- RespSinistres.declare requires OpMembres.admis via RespSinistres.op_de (regles.toml ; arête déjà prise par le when sur la police) ; + calls ApiOperateurs.statut (refus NonEligible).
Structure : état RespFautes.designation_presumee (aucune -> …, … -> contradictoire, … -> ecartee) ; slot RespSinistres.op_de (écrit une fois par declarer) ; regles.toml créé (2 tables).
Écartées :
- EvtEvenements.confirme ⇐ ref objets catalogués non rentrés : lien = liste de 1-2 objets, et « non rentré » = 4 états RefObjet (≥ 3 alternatives) ; contrôle du corps (signaler refuse Inconnu).
- EvtEvenements.clos ⇐ dossier gelé : dossier_de écrit par 2 signaux hors création, seulement pour les collisions ; un nouvel état casserait les `when EvtEvenements.confirme || clos` de ref et conjonctions → reportée sur EvtCollisions.transmise.
- Observation « suivi < 72 h » pour la levée : aucune machine par objet chez observation (contrôle du corps, ApiObservation.suivi).
- EvtNotifications.envoyee ⇐ ApiDonnees.peut_recevoir : fonction calculée (diffusion, embargo, accord), pas un état ; destinataire écrit par 3 signaux.
- RespDossiers.leve while rapprochements clos : plusieurs cibles par dossier (contrôle du corps).
- « responsabilite:purge ⇐ CoordDecision.purgee ou jamais prise » (coordination) : inverse de la règle détenteur ⇐ RespPurges.autorisee, « jamais prise » n'est pas un état.
Verdicts : check app.cell 0 erreur ; cycles.py sans composante, profondeur max 23 ; verify (1 run, copie avec regles.toml fusionné) VERIFY OK, aucun ✗ ni « not explored » ; un seul ⚠ sur mes règles : open obligation RespSinistres.op_de écrit par le trou declarer (acceptable).

## Phase 3 — corps (14/11/2026)
- Signaux remplis : 75/75 (8 cells internes, 64 + façade ApiEvenements 11) ; ~1 260 lignes de corps (sys/evenements/chair/*.cell, 9 fichiers) ; banc_evenements.cell : check 0 erreur, 0 avertissement sur mes fichiers.
- Tests : test_evenements.cell, 18 cells de test / 275 assertions, toutes vertes (`$S test test_evenements.cell`). Scénarios : fragmentation (signalement → estimation → confirmation 4 yeux → 7 notifications → cause → fragment détecté/rattaché/catalogué/rentré → ronde dissipe → clos) ; collision décision ignorée (constat figé → verdict 4 yeux → transmission → nuage stabilisé par décision → clos) ; décision suivie (manœuvre détectée, plan exécuté) ; hors coordination ; infirmation ; anomalies (levée, rejet, perte, close par fragmentation) ; notifications (retenue, acquittée, sans réponse) ; veille ; nuages ; fragments ; rapports (plafond de diffusion).
- Doublures : doublures/evenements.cell (ApiAcces, ApiRef, ApiConjonctions, ApiObservation, ApiCoordination, ApiManoeuvres, ApiResponsabilite, ApiDonnees, ApiPasserelle, ApiConformite, ApiOperations, ApiOperateurs, ApiEphemerides), sans état, conventions en tête du fichier.
- Cadres corrigés (amendés aussitôt, `check app.cell` 0 erreur) :
  1. EvtEvenements.clore : + calls EvtNuages.etat (l'arête `when EvtNuages.dissipe` n'avait aucun getter pour être requise).
  2. EvtCollisions.instruire : + calls EvtEvenements.etat (`when EvtEvenements.confirme` non vérifiable : une instruction est "constatee" dès le signalement).
  3. EvtRapports.rediger : + calls EvtEvenements.etat (même raison ; un évènement clos ne doit plus ouvrir de rédaction).
  4. EvtFragments.ecarter : + reads nuage_du_fragment (refus Inconnu impossible sans lui).
  5. EvtEvenements.geler_preuves : reads genre_de, evenement (refus Inconnu).
  Les `when` internes sans getter (EvtNuages.ouvrir/abandonner, EvtCollisions.classer, EvtRapports.classer) sont garantis par l'appelant unique, qui fait sa transition avant l'appel.
- Demandes à l'intégrateur : aucune nouvelle (celles de 1b restent : retrait de `use sys::evenements::abonnements` dans app.cell).
- Défauts de conception trouvés : (a) EvtCollisions.transmettre notifie le fait "collision", déjà notifié sous la même clé à la confirmation : la dédoublonnage par `par_cle` l'avale (il faudrait un fait "transmission" dans EvtSujet) ; (b) EvtCollisions.instruire n'a pas l'instant de signalement de la coordination : la recherche de manœuvre part de TCA − 7 j ; (c) EvtEvenements.en_souffrance juge « collision sans dossier » depuis le signalement (aucun slot n'a l'heure de confirmation) ; (d) Inconnu de ApiEphemerides.invalider (aucun dépôt) est toléré à la prise en charge.
- Trous laissés : aucun. La ronde enveloppe chaque étape dans `try` (une étape en échec compte 0, la ronde ne se bloque pas) ; le journal n'est écrit que si la ronde a changé quelque chose.
- Limites de Soma rencontrées : types nominaux stricts (Niveau/Gravite/Horodatage/EvtSujet à reconstruire explicitement) ; `get_status` d'un id jamais vu renvoie l'état initial, pas `()` (d'où les refus Inconnu par slot) ; invariants entre slots imposent un ordre d'écriture (fin avant début, haute avant basse — basse remise à 0 d'abord) ; un `try` sur `slot.set` sert à traduire une violation d'invariant en refus métier (compter_rentree).
