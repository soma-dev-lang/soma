# responsabilite — notes d'architecture

## Choix de conception
- 6 fichiers, 12 cells, 10 machines : `dossiers` (RespDossiers, RespGels), `pieces` (RespPieces, RespCollecte), `reclamations` (RespReclamations, RespCommissions — Convention de 1972), `fautes` (RespFautes), `assurance` (RespPolices, RespSinistres), `conservation` (RespDurees, RespPurges, RespCommunications).
- Propriété 6, traçabilité : un dossier gèle des PIÈCES (instantané daté + empreinte) collectées dans les façades par RespCollecte ; `RespPieces.fonder` relie une pièce de décision (coordination, manœuvre) aux pièces de données qui la fondaient, jamais à une donnée postérieure ; `RespPieces.chaine` rend la chaîne de preuve. Une pièce ne se gèle que dans un dossier `gele` (`when RespDossiers.gele via dossier_de`) ; le scellement exige toutes les pièces vérifiées et fige une empreinte globale ; une altération déclenche un incident de gravité 3.
- Propriété 6, diffusion : une communication de pièces n'est autorisée que sur dossier scellé (`when RespDossiers.scelle via dossier_de`), après `ApiDonnees.peut_recevoir` pour chaque objet de chaque pièce, recontrôlée à l'envoi, chaque envoi tracé par `ApiDonnees.tracer`.
- Gel : RespGels (id = cible, rapprochement ou objet) est réactif (libre ⇄ gele, compteur de dossiers). Une purge n'est autorisée ni constatée que sur cible libre (`when RespGels.libre via cible_de`) ; poser un gel annule les purges autorisées en attente.
- Une qualification de faute « retenue » n'est possible que sur dossier scellé ; une commission ne se demande que sur dossier scellé ; RespCommissions et RespReclamations se tiennent mutuellement (même id) ; un sinistre ne s'indemnise qu'après réclamation réglée ou close.
- Montants : type local `RespMontant` (euros entiers) ; délais de la Convention en constantes (1 an, 2 et 4 mois).

## Règles entre sous-systèmes proposées
- Cible = donnees/ephemerides/conjonctions/coordination:<purge exécutée> ; exige = responsabilite:RespPurges.autorisee ; lien = id de purge (slot à écrire une fois par le détenteur).
- Cible = RespDossiers.leve ; while = conjonctions:rapprochement clos (resolu|passe|collision) ; lien = chaque cible du dossier (non exprimable : plusieurs cibles — contrôle du corps).
- Cible = RespFautes.presumee (nature designation_ignoree) ; exige = coordination:arretee ; lien = RespFautes.rapprochement_de.
- Cible = RespFautes.retenue ; exige = conjonctions:rapprochement passe ou collision (TCA échu) ; lien = RespFautes.rapprochement_de.
- Cible = RespSinistres.declare ; exige = operateurs:opérateur admis ; lien = RespPolices.op_de (via police_de : chaîne de deux liens, à aplatir).
- Cible = coordination:arretee / manoeuvres:approuve ; exige = trace journalisée des données de décision (pc, éphémérides, version de règles) — c'est ce que RespCollecte relit ; à défaut les pièces portent "absent=<tag>".

## Phase 1b — contrat v2 (14/11/2026)
- Façade branchée 5/5 (facade.txt) : geler → RespDossiers.ouvrir ; sous_gel → RespGels.est_gele ; purge_autorisee → RespPurges.autorisee_pour ; chaine → RespPieces.chaine_de ; balayer → RespRondes.balayer (nouvelle cell, 13 cells / 10 machines).
- Horloge : aucun `every`/`now` (il n'y en avait pas) ; les rondes sont RespPieces.ronde (revérification des empreintes tous les 30 j), RespPolices.echoir (devenu une ronde), RespFautes.relancer_contradictoires, RespCommissions.relancer_carences (remplace en_carence).
- Trous fermés : `ouvrir` vérifie ApiEvenements.existe et gèle seul l'évènement, son jour "J:<jour>", ses objets (ApiEvenements.objets), leurs rapprochements des 7 j (ApiConjonctions.historique) et leurs plans (ApiManoeuvres.plans_de) ; "systeme" admis. Pièces à l'instant de la décision : ApiEphemerides.fondement/empreinte/depot_valide, ApiRef.valeur_au, ApiCoordination.decision, ApiPriorites.decision_de ; chaque pièce porte la `reference` source (index chaine_de). Réclamation : défendeur ∈ ApiLancements.etats_de_lancement (NonEligible). designe(r, "systeme") ; notifier avec r.
- Complément : RespDossiers.completer ouvre un dossier lié (parent_de) à un dossier scellé ; lever lève aussi les compléments.

## Phase 1b — demandes
- api.cell : `chaine` porte `calls RespGels.est_gele` → remplacer par `calls RespPieces.chaine_de` ; ajouter `calls RespGels.est_gele` à `sous_gel`, `calls RespPurges.autorisee_pour` à `purge_autorisee`, `calls RespRondes.balayer` à `balayer`.
- Règle complément non exprimable : `aucun -> gele` est pris par ouvrir (sans condition) et completer (parent scellé) ; la condition `RespDossiers.scelle via parent_de` reste un contrôle du corps.

## Points ouverts
- check app.cell : 0 erreur ni avertissement sur mes cells hors les invariants entre slots (seeding `?? 0`, corps : slot de droite d'abord) ; les 3 erreurs restantes sont dans conjonctions et meteo (en cours d'écriture).
- Lignes : 1 066 de squelette interne (compléments, rondes, références de pièces).

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
- Signaux remplis : 102/102 (13 cells + façade ApiResponsabilite 5/5), 6 helpers privés ; 1 768 lignes de corps (sys/responsabilite/chair/*.cell), doublures 243 lignes (19 façades), tests 545 lignes.
- Tests : `$S test test_responsabilite.cell` → 13 cells de test, 336 assertions, toutes vertes. Couverts : cycle dossier (ouverture + gels auto évènement/jour/objets/rapprochements/plans, 22 pièces, scellement 4 yeux, levée après 10 ans), compléments, rattachements, annulation (Conflit sur faute pendante), pièces (déclaration, fondements antérieurs, chaîne, absence prouvée "absent=Perime", ronde), durées + purges (annulation par gel, Conflit, Delai), fautes (6 natures, contradictoire, qualification sur dossier scellé, transmission, ronde), scénario Convention 1972 complet (faute retenue → réclamation espace → commission → force obligatoire → sentence → clôture → sinistre indemnisé), règlement amiable, carence de commission (siège et président pourvus par ORBITE, prorogation, retrait), polices, sinistres, communications (Insuffisant, Confidentiel, traces, révocation).
- Cadres amendés (2 `--amend`) : RespDossiers + slot `gels_de` (toutes les cibles gelées par un dossier ; écrit par ouvrir/completer/rattacher_*, lu par annuler/lever — sans lui l'évènement, le jour et les plans restaient gelés à jamais) ; completer + writes objets_de, rapprochements_de ; empreinte + reads dossier (distinguer Inconnu de « non scellé ») ; RespReclamations.clore + reads demandeur_de, defendeur_de (aviser les deux États d'une sentence obligatoire inexécutée) ; RespCommissions.designer_membre + reads commission (EtatInvalide après constitution/dissolution) ; frames resserrés (annuler, lever, RespPieces.chaine_de).
- Demandes à l'intégrateur : aucune bloquante. À trancher : RespCommissions.saisir ne peut pas contrôler `when RespReclamations.commission` (aucun getter dans ses calls ; ajouter `calls RespReclamations.etat` ferait un aller-retour Reclamations→Commissions→Reclamations) — le corps fait confiance à son seul appelant, qui transitionne avant d'appeler.
- Choix : portée des rôles d'ORBITE assurée par ApiAcces.a_le_role (portée incluse) ; "systeme" admis par ouvrir pour tout évènement existant (escalade, operations, reentrees… appellent geler en systeme), pas seulement les collisions ; vérification quatre yeux avant le rôle du valideur (QuatreYeux prime) ; une pièce de données est datée à l'instant de la décision (ApiCoordination.decision, champ 5), la manœuvre après elle, le journal à h.
- Limites/défauts : branche « altérée » de verifier écrite mais non testable (contenu_de jamais réécrit) ; lever ne contrôle pas « rapprochements clos » (pas d'ApiConjonctions.etat dans son cadre ; sans portée après 10 ans) ; collecter_plan ne connaît pas le rapprochement d'un plan (façade ApiManoeuvres) : il se fonde sur la décision seulement si le plan était déjà figé (chaine_de(plan)).
- Pièges Soma rencontrés : `transition(id, st, cible)` avec cible calculée refusé par check (écrire chaque arête) ; types nominaux à reconstruire (`Etat(pts[0])`, `RespLieuDommage(lieu)`) ; `parse_int` (strict) plutôt que `to_int` pour lire un instant dans une chaîne.
