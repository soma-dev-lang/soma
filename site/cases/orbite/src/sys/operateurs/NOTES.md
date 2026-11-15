# operateurs — notes d'architecture

## Choix de conception
- 7 fichiers, 9 cells (dont OpRondes, sans machine, en fin d'adhesion.cell), 8 machines : OpDossiers (dossier), OpMembres (adhesion, id = Org), OpLicences (licence, id = Org),
  OpEngagements (engagement, id = Org), OpAstreinte (creneau), OpExercices (exercice), OpChangements (changement), OpContacts (contact).
- Chaîne d'entrée prouvée : dossier `-> accepte when OpLicences.en_vigueur via operateur` ; adhésion `candidat -> admis when
  OpDossiers.accepte via dossier_de` ; `admis -> actif when OpEngagements.en_vigueur` (même id) ; créneau `-> en_service` et exercice
  `-> lance when OpEngagements.en_vigueur via operateur` ; changement `-> effectif when OpMembres.actif || OpMembres.suspendu via operateur`.
- est_actif = statut "actif" ET licence valide à h (lue à chaque appel) : une licence échue coupe l'opérateur sans balayage.
- delai_reponse : sans engagement en vigueur, les plafonds (1 h / 12 h / 72 h) ; invariants n3 ≤ n2 ≤ n1, bornes [5 min, plafond].
- Astreinte : un créneau ne compte qu'une fois PRIS par la personne ; manque, interruption ou fin sans relève ⇒ astreinte = "" ⇒
  escalade immédiate ; trou > 24 h ⇒ suspension auto (origine "joignabilite"). 3 échecs d'exercice consécutifs ⇒ idem.
- joignable = exercice réussi < 31 j (accusé par la personne EN SERVICE, pas n'importe quel employé) ET astreinte en service.
- Suspension : origine typée ; une suspension d'origine "etat" ne se lève que par l'État. Le retrait exige un catalogue sans objet en
  orbite ; l'exclusion laisse les objets à l'État de tutelle (OpMembres.repondant ne rend jamais "").

## Règles entre sous-systèmes proposées
- Cible = OpLicences.en_vigueur ; exige = ref:État de la licence participant ; lien = OpLicences.etat_licence (réécrit au transfert : à figer)
- Cible = OpMembres.actif ; exige = ref:organisation existante non radiée ; lien = même id (Org)
- Cible = OpDossiers.accepte ; exige = donnees:adhérent à l'accord de partage ; lien = OpDossiers.operateur
- Cible = OpExercices.reussi ; exige = passerelle:message d'exercice acquitté ; lien = OpExercices.message_de
- Cible = OpAstreinte.en_service ; exige = acces:personne active ("dynamicien" de l'opérateur) ; lien = OpAstreinte.personne
- Cible = OpChangements.approuve ; exige = ref:État d'accueil participant ; lien = OpChangements.etat_accueil

## Avertissements expliqués
- check : invariants entre slots (debut/echeance, n3/n2/n1, lance_a/echeance) : écrire d'abord le slot de droite ; `len(resultats) <= 64`
  borne voulue de chaque liste ; `request` de ApiPasserelle : hors périmètre.

## Phase 1b — contrat v2
- Façade branchée 10/10 (facade.txt) : actifs → OpMembres.actifs(h, n) (statut ET licence valide à h) ; contacts →
  OpContacts.contacts_echelon (1 astreinte, 2 chef_de_mission puis suppléants, 3 point_focal de l'État de tutelle) ; balayer → OpRondes.balayer.
- Horloge : il n'y avait ni every ni now ; OpRondes.balayer enchaîne licences, créneaux, exercices (échus, dus à lancer, à planifier),
  contacts, changements caducs ; rondes lentes (alerte planning 1 h, exercices et renouvellements 1 j) mémorisées dans dernier_passage.
- Trous fermés : dépôt refusé NonEligible si ApiRef.genre(op) ≠ "operateur" ; changement de contrôle effectif → ApiRef.changer_tutelle
  (rendre_effectif prend désormais superviseur + valideur direction) avant le transfert de licence.
- notifier(…, r, …) : r = "" pour tous les messages d'operateurs (aucun n'est lié à un rapprochement).
## Phase 1b — demandes
- Échelon 3 : acces n'expose pas la liste des personnes d'un rôle dans une Org (actifs_de ne rend qu'un nombre) ; operateurs tient donc un
  registre de "point_focal" déclarés par l'autorité de l'État (OpContacts, place "point_focal"). Si acces ajoute
  `ApiAcces.personnes_de(org: Org, r: Role) -> List<String>`, contacts_echelon(…, 3, …) pourra le croiser avec ce registre.

## Phase 2 — règles entre sous-systèmes (operateurs + passerelle)
- Écrites (4) :
  1. OpChangements.approuve ⇐ RefEtat.participant via etat_accueil — arête (approuver ; + ApiRef.etat_participant).
  2. OpExercices.reussi ⇐ PasMessages.acquitte via message_de — arête (accuser ; + ApiPasserelle.acquitte, reads message_de).
  3. PasCanaux.actif ⇐ RefOrg.inscrite || RefOrg.active via org_de — arête (verifier ; + ApiRef.organisation_existe).
  4. OpAstreinte.en_service ⇐ AccesPersonnes.active via OpAstreinte.personne — toml (sys/operateurs/regles.toml : arête déjà `when OpEngagements.en_vigueur`).
- Verdicts (verify ×2 sur une copie avec regles.toml fusionné) : les 4 en « open obligation … written by the hole » (lien écrit par
  declarer/lancer/planifier, jugé au corps) ; aucun ✗ ni « not explored » sur operateurs/passerelle. Seul ✗ du run : EscVeille.couverte (escalade, hors périmètre).
  check 0 erreur ; cycles.py : aucune composante, profondeur max 23.
- Écartées :
  - OpMembres.actif ⇐ RefOrg.active : toml essayé → ⚠ « a run enters actif without it » (activer depuis RefOrg=inscrite : cadre de trou) ;
    admis->actif porte déjà un when, suspendu->actif est un retour arrière. Contrôle au corps : ApiRef.organisation_existe ajouté à activer/retablir.
  - OpLicences.en_vigueur ⇐ RefEtat.participant via etat_licence : lien réécrit par declarer et transferer (pas écrit une fois).
  - OpDossiers.accepte ⇐ donnees:adhérent : arête déjà prise ; adhésion "AD-n" inconnue au dépôt et renouvelée sous un nouvel id ; corps : ApiDonnees.adherent.
  - OpMembres.actif ⇐ PasCanaux.actif : canal principal variable, choisi par l'org, aucun lien écrit une fois.
  - Suspension "conformite" ⇐ ConfDemande.acceptee, retablir ⇐ ConfMesure.levee||echue : suspensions répétées, aucun lien op → demande/mesure écrit une fois.
  - PasMessages.escalade ⇐ escalade ouverte : dossier "ESC-n" inconnu de passerelle (escalader rend l'échelon), et e1|e2|e3|e4 = 4 alternatives.
- Structure : aucun état ni slot ajouté ; calls et commentaires (Refus EtatInvalide) mis à jour sur approuver, accuser, verifier, activer, retablir, prendre_service.

## Phase 3 — corps
- Signaux remplis : 87/87 (9 cells internes + façade ApiOperateurs, 0 trou) ; 1 333 lignes de corps (sys/operateurs/chair/*.cell, 8 fichiers), 1 helper privé (OpExercices._garder_).
- Tests : test_operateurs.cell, 16 cells de test, 267 assertions, toutes vertes (`$S test test_operateurs.cell`) ; cycles de vie des 8 machines, refus Habilitation / QuatreYeux / portée (autre État, autre employeur) / état, scénario métier « astreinte perdue → échelon 1 vide → répondant = État → suspension auto après 24 h → rétablissement conformité », changement de contrôle ALD → BRV jusqu'à la licence reconfirmée par l'État d'accueil.
- Doublures (doublures/operateurs.cell, sans mémoire) : ApiAcces (a_le_role, agit_pour, quatre_yeux), ApiRef (orbite, etat_participant, organisation_existe, etat_de, org_de_etat, genre, objets_de, etat_objet, changer_tutelle), ApiPasserelle (notifier, acquitte), ApiConformite.constater, ApiDonnees.adherent. Personne "<role>.<nom>@<ORG>", État de tutelle = dernier segment de l'Org.
- Cadres corrigés (amendés, `skeleton app.cell --amend`) : OpMembres invariant `(candidat_depuis ?? 0) <= (admis_a ?? 9e18)` (l'ancien `?? 0` refusait toute inscription de candidat) ; reads candidat_depuis sur OpMembres.retirer (Inconnu, et ne pas créer un « retire » pour un jamais-candidat) ; reads operateur sur OpDossiers.instruire (Inconnu) ; calls OpMembres.etat sur OpLicences.echoir (« manquement s'il est actif ») ; reads numero sur repondre_etat / suspendre_par_etat / lever_suspension / retirer_par_etat (historique complet) ; reads creneau sur OpAstreinte.planifier (Conflit sans compter les créneaux annulés) ; reads lance_a sur OpExercices.accuser (délai de réponse du bilan) ; reads operateur sur OpChangements.approuver (notifier l'opérateur) ; reads contact sur OpContacts.declarer (Doublon hors contacts retirés).
- Demandes à l'intégrateur : aucune bloquante. (Rappel phase 1b : `ApiAcces.personnes_de(org, r)` pour croiser l'échelon 3 avec acces.)
- Défauts de conception relevés : (1) édition 2026 : get_status rend l'état initial, jamais () — « Inconnu » ne se distingue plus de l'état initial que par un slot ; activer/suspendre/retablir/exclure rendent donc EtatInvalide pour une Org connue jamais admise, Inconnu seulement si l'Org est absente du référentiel. (2) OpAstreinte.en_service lève Inconnu pour un opérateur jamais planifié : joignable, accuser et contacts_echelon(1) l'encapsulent dans un try (→ false / []). (3) interrompre ne lit pas debut : une interruption avant le début prévu (prise anticipée) garde la fin prévue (écriture sous try). (4) La planification d'exercice tire l'instant de façon déterministe (h, longueur de l'id) faute de `random` dans le cadre de OpRondes.balayer. (5) suspendre_auto ne constate un manquement (gravité 3) que pour l'origine "joignabilite" : pour "licence", retirer_par_etat / echoir le font déjà.
- Limites de Soma rencontrées : `transition(id, st, cible)` avec une source variable est refusé par check quand une arête hors cadre mène à la cible (déclarer : if explicite) ; types nominaux (Org/Etat/Horodatage) : comparaisons et arithmétique via to_int / to_string / Org(…) ; `e.value` d'une Map n'est pas typé (avertissement, sans effet) ; un invariant entre slots exige un ordre d'écriture (n1→n2→n3 au premier accord, max/min intermédiaires à la révision).
