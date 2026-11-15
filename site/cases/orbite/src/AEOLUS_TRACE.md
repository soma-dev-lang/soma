# Rejeu Aeolus / Starlink 44 dans ORBITE COMMUNE — la trace

**Ce qui est historique, ce qui est simulé.** Seuls quatre faits viennent de l'incident public de
septembre 2019, tels que rapportés par l'ESA et SpaceX : le 28 août 2019, une alerte de rapprochement
entre Aeolus (ESA) et Starlink 44 (SpaceX) donne une probabilité de collision d'environ 1/50 000, sous le
seuil de manœuvre, et SpaceX indique par courriel ne pas prévoir de manœuvrer à ce niveau ; les jours
suivants, la probabilité monte jusqu'à environ 1/1 000, dix fois le seuil de manœuvre de l'ESA
(1/10 000), et l'ESA n'obtient plus de réponse (un défaut du système d'astreinte — paging — de SpaceX a
empêché l'opérateur de voir les messages suivants) ; le 2 septembre 2019, Aeolus relève seule son orbite
environ une demi-orbite avant le rapprochement ; aucune règle internationale ne disait qui devait
manœuvrer. **Tout le reste est simulé** : ORBITE elle-même (rien de tel n'existait en 2019), ses États
fictifs (ALD siège, BRV tutelle de l'ESA, CAZ tutelle de SpaceX), ses personnes, ses règles PRIO-2019 et
sa table de seuils SEU-1, les heures (alerte le 28/08 à 10 h, montée le 30/08 à 14 h, TCA le 02/09 à
12 h UTC), distances (4 000 m puis 150 m), masses, vecteurs d'état, delta-v (0,5 m/s), capteurs, numéros
de catalogue (OBJ-1 = OBJ-AEOLUS, OBJ-2 = OBJ-STARLINK44 : ref numérote lui-même), l'acceptation de la
proposition par l'ESA, les flux automatiques d'éphémérides et de mesures, et l'horloge (un battement
toutes les 30 min). Les pc sont en milliardièmes (20 000 = 1/50 000 ; 1 000 000 = 1/1 000).

Source : `Journal.chercher` sur le rapprochement `CJ-1`, le dossier d'escalade `ESC-1`/`ESC-2`, le plan
`PL-AEOLUS`, la détection `MD-1`, les éphémérides et orbites d'AEOLUS, le manquement `MQ-15`, les
règles `PRIO-2019` et les seuils `SEU-1` — test `test_aeolus.cell` (A5Evitement), vrais corps, sans
doublure. Lignes omises : relances horaires d'éphémérides, suivis servis, messages non acquittés versés
à l'escalade en cours, rappels horaires « manquement grave en souffrance ». Horodatages UTC ; les
valeurs numériques des évènements (TCA, échéances, fenêtres) sont en ms depuis l'époque Unix
(1567425600000 = 02/09/2019 12:00:00, TCA ; 1567396800000 = 02/09/2019 04:00:00, échéance finale).

## En bref

| Étape | Quand (UTC) | Ce qu'ORBITE fait |
|---|---|---|
| Alerte à 1/50 000 | 28/08 10:02 | rapprochement CJ-1 de niveau 1, ni coordination ni désignation |
| Montée à 1/1 000 | 30/08 14:02 | niveau 3, coordination ouverte ; les règles désignent AEOLUS (« seul_capable » : la propulsion faible de STARLINK44 a un préavis de 72 h, le TCA est à 70 h) |
| L'ESA accepte | 30/08 14:10 | — |
| Silence de SpaceX | 30/08 15:00 | manquement MQ-15 (réponse tardive, gravité 3) ; escalade ESC-1 échelon 1 (astreinte) |
| Escalade | 15:30 / 16:00 / 31/08 01:00 | échelon 2 (chef de mission), 3 (point focal de CAZ), 4 (direction d'ORBITE, points focaux de BRV et CAZ) |
| Reprise par ORBITE | 02/09 04:00 | TCA − 8 h (préavis d'AEOLUS 6 h + 2 h d'arbitrage) ; coordination en arbitrage |
| Arbitrage à quatre yeux | 02/09 04:05 | ana1 + sup1 : AEOLUS manœuvre (décision DS-CJ-1-1) |
| Plan d'évitement | 02/09 04:20 | PL-AEOLUS déclaré, criblé sûr, approuvé (poussée prévue à 11:15) |
| Poussée | 02/09 11:15:10 | TCA − 45 min, « environ une demi-orbite » |
| Vérification | 02/09 11:50 | orbite observée conforme ; CJ-1 résolu, coordination close |

## Chronologie (journal d'ORBITE, ligne à ligne)

| Horodatage (UTC) | Sous-système | Acteur | Évènement |
|---|---|---|---|
| 01/08/2019 00:01:00 | priorites | systeme | version PRIO-2019 en vigueur (remplace '') |
| 01/08/2019 10:00:00 | conjonctions | ana1 | seuils SEU-1 rédigés : seuil de manœuvre 1/10 000 comme seuil d'attention |
| 01/08/2019 10:16:40 | conjonctions | sup1 | seuils SEU-1 adoptés (valideur dir1) |
| 28/08/2019 10:02:00 | conjonctions | systeme | rapprochement CJ-1 (OBJ-1\|OBJ-2) ouvert : TCA 1567425600000, 4000 m, pc 20000, niveau 1 |
| 30/08/2019 14:02:00 | conjonctions | systeme | alerte de CJ-1 : 4 messages préparés |
| 30/08/2019 14:02:00 | conjonctions | systeme | rapprochement CJ-1 (OBJ-1\|OBJ-2) mis à jour : TCA 1567425600000, 150 m, pc 1000000, niveau 3 |
| 30/08/2019 14:02:00 | conjonctions | systeme | rapprochement CJ-1 : suivi renforcé demandé (["TS-1", "TS-2"]) |
| 30/08/2019 14:02:00 | conjonctions | systeme | rapprochement CJ-1 en coordination (CO-CJ-1, proposee) |
| 30/08/2019 14:02:00 | coordination | systeme | coordination CJ-1 ouverte : proposee, propose 'OBJ-1', raison '' |
| 30/08/2019 14:02:00 | observation | systeme | tâche TS-1 demandée : OBJ-1, priorité 3, rapprochement (rapprochement CJ-1 niveau 3) |
| 30/08/2019 14:02:00 | observation | systeme | tâche TS-2 demandée : OBJ-2, priorité 3, rapprochement (rapprochement CJ-1 niveau 3) |
| 30/08/2019 14:02:00 | priorites | systeme | décision PD-1 sur CJ-1 : 'OBJ-1' (seul_capable) sous PRIO-2019 |
| 30/08/2019 14:10:00 | coordination | dyn.esa | ORG-ESA accepte OBJ-1 sur CJ-1 (tour 1) |
| 30/08/2019 14:30:00 | escalade | systeme | veille armee sur CJ-1 |
| 30/08/2019 15:00:00 | conformite | systeme | manquement MQ-15 constate : ORG-SPACEX reponse_tardive g3 reference CJ-1 |
| 30/08/2019 15:00:00 | conformite | systeme | manquement MQ-15 grave notifie a la conformite d'ORBITE |
| 30/08/2019 15:00:00 | escalade | systeme | dossier ESC-1 ouvert contre ORG-SPACEX sur CJ-1 a l'echelon 1 (echeance finale 1567396800000) : silence coordination |
| 30/08/2019 15:00:00 | escalade | systeme | relance REL-1 (complete) emise vers ORG-SPACEX pour ESC-1 |
| 30/08/2019 15:00:00 | passerelle | systeme | message MSG-50 (manquement grave MQ-15) : défaut journalisé, sans nouveau manquement (ORG-ORBITE) |
| 30/08/2019 15:30:00 | escalade | systeme | dossier ESC-1 monte a l'echelon 2 : delai de l'echelon ecoule |
| 30/08/2019 15:30:00 | escalade | systeme | relance REL-2 (complete) emise vers ORG-SPACEX pour ESC-1 |
| 30/08/2019 16:00:00 | escalade | systeme | dossier ESC-1 monte a l'echelon 3 : delai de l'echelon ecoule |
| 30/08/2019 16:00:00 | escalade | systeme | relance REL-3 (complete) emise vers ORG-CAZ pour ESC-1 |
| 31/08/2019 01:00:00 | escalade | systeme | concertation ESC-1 convoquee (BRV,CAZ) |
| 31/08/2019 01:00:00 | escalade | systeme | dossier ESC-1 monte a l'echelon 4 : delai de l'echelon ecoule |
| 31/08/2019 01:00:00 | escalade | systeme | relance REL-4 (complete) emise vers ORG-ORBITE pour ESC-1 |
| 31/08/2019 01:00:00 | escalade | systeme | relance REL-5 (complete) emise vers ORG-BRV pour ESC-1 |
| 31/08/2019 01:00:00 | escalade | systeme | relance REL-6 (complete) emise vers ORG-CAZ pour ESC-1 |
| 02/09/2019 04:00:00 | coordination | systeme | CJ-1 : silence de 1 partie(s), arbitrage d'ORBITE (ORG-ORBITE) |
| 02/09/2019 04:00:00 | coordination | systeme | saisine CJ-1#1 : silence |
| 02/09/2019 04:00:00 | escalade | systeme | demarche DIP-1 preparee vers CAZ pour ORG-SPACEX (dossier ESC-1, 1 escalades graves) |
| 02/09/2019 04:00:00 | escalade | systeme | dossier ESC-1 : reprise par ORBITE sur CJ-1 |
| 02/09/2019 04:00:00 | escalade | systeme | gel de ESC-1 refuse : Inconnu |
| 02/09/2019 04:00:00 | escalade | systeme | prise en charge de ESC-1 ouverte, limite 1567397700000 (notifiee) |
| 02/09/2019 04:05:00 | coordination | ana1 | CJ-1 arbitrée : 'OBJ-1', décision DS-CJ-1-1, valideur sup1 |
| 02/09/2019 04:05:00 | coordination | ana1 | verrou CJ-1 posé : décision DS-CJ-1-1, désigné 'OBJ-1', mode arbitrage, valideur 'sup1' |
| 02/09/2019 04:15:00 | ephemerides | dyn.esa | depot EPH-AEOLUS-P de OBJ-1 valide (sans_temoin, qualite faible) |
| 02/09/2019 04:15:00 | ephemerides | systeme | depot EPH-AEOLUS-P courant pour OBJ-1 |
| 02/09/2019 04:20:00 | conjonctions | systeme | plan PL-AEOLUS (OBJ-1, dv 500 mm/s) criblé sur CB-66 : sur (0) |
| 02/09/2019 04:20:00 | manoeuvres | dyn.esa | plan PL-AEOLUS déclaré (evitement) sur OBJ-1, fenêtre 1567422900000-1567426500000, dv 500 |
| 02/09/2019 04:20:00 | manoeuvres | systeme | criblage du plan PL-AEOLUS : 0 conflit(s), dépôt EPH-AEOLUS-P |
| 02/09/2019 04:20:00 | manoeuvres | systeme | plan PL-AEOLUS approuvé (verrou CJ-1, dépôt EPH-AEOLUS-P) |
| 02/09/2019 04:20:00 | manoeuvres | systeme | plan PL-AEOLUS criblé sûr |
| 02/09/2019 04:30:00 | escalade | systeme | concertation ESC-1 manquee (dossier close) |
| 02/09/2019 04:30:00 | escalade | systeme | dossier ESC-1 clos (decision_arretee) : decision arretee constatee au balayage |
| 02/09/2019 04:30:00 | escalade | systeme | prise en charge de ESC-1 tranchee |
| 02/09/2019 04:30:00 | escalade | systeme | veille CJ-1 couverte |
| 02/09/2019 06:00:00 | operations | systeme | gel d'urgence armé pour CJ-1 (TCA 1567425600000) |
| 02/09/2019 09:30:00 | escalade | systeme | demarche DIP-2 preparee vers CAZ pour ORG-SPACEX (dossier ESC-2, 2 escalades graves) |
| 02/09/2019 09:30:00 | escalade | systeme | dossier ESC-2 : reprise par ORBITE sur CJ-1 |
| 02/09/2019 09:30:00 | escalade | systeme | dossier ESC-2 ouvert contre ORG-SPACEX sur CJ-1 a l'echelon 4 (echeance finale 1567396800000) : avis de manoeuvre PL-AEOLUS\|ORG-SPACEX\|approbation sans accusé |
| 02/09/2019 09:30:00 | escalade | systeme | gel de ESC-2 refuse : Inconnu |
| 02/09/2019 09:30:00 | escalade | systeme | prise en charge de ESC-2 ouverte, limite 1567417500000 (notifiee) |
| 02/09/2019 09:30:00 | escalade | systeme | prise en charge de ESC-2 tranchee |
| 02/09/2019 09:30:00 | escalade | systeme | relance REL-7 (complete) emise vers ORG-ORBITE pour ESC-2 |
| 02/09/2019 09:30:00 | escalade | systeme | relance REL-8 (complete) emise vers ORG-BRV pour ESC-2 |
| 02/09/2019 09:30:00 | escalade | systeme | relance REL-9 (complete) emise vers ORG-CAZ pour ESC-2 |
| 02/09/2019 09:30:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-BRV\|approbation : escalade refusée (Inconnu) |
| 02/09/2019 09:30:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-BRV\|approbation échu sans accusé |
| 02/09/2019 09:30:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-SPACEX\|approbation échu sans accusé |
| 02/09/2019 10:00:00 | escalade | systeme | dossier ESC-2 clos (decision_arretee) : decision arretee constatee au balayage |
| 02/09/2019 11:20:00 | manoeuvres | dyn.esa | plan PL-AEOLUS exécuté à 1567422910000, dv 500 |
| 02/09/2019 11:30:50 | observation | ana1 | détermination OD-1 calculée pour OBJ-1 (6 mesures, 2 capteurs) |
| 02/09/2019 11:31:00 | observation | ana1 | détermination OD-1 validée pour OBJ-1 (qualité haute, écart 3500 m) |
| 02/09/2019 11:31:00 | observation | systeme | détection MD-1 ouverte sur OBJ-1 : écart 3500 m (OD-1) ; suivi TS-1 |
| 02/09/2019 11:31:40 | observation | ana1 | détection MD-1 confirmée |
| 02/09/2019 11:31:40 | observation | systeme | détection MD-1 expliquée par PL-AEOLUS |
| 02/09/2019 11:35:00 | ephemerides | dyn.esa | depot EPH-AEOLUS-2 de OBJ-1 valide (conforme, qualite faible) |
| 02/09/2019 11:35:00 | ephemerides | systeme | depot EPH-AEOLUS-2 courant pour OBJ-1 |
| 02/09/2019 11:45:50 | observation | ana1 | détermination OD-2 calculée pour OBJ-1 (6 mesures, 2 capteurs) |
| 02/09/2019 11:46:00 | observation | ana1 | détermination OD-2 validée pour OBJ-1 (qualité haute, écart 0 m) |
| 02/09/2019 11:50:00 | conjonctions | systeme | cloture de CJ-1 : 2 messages préparés |
| 02/09/2019 11:50:00 | conjonctions | systeme | rapprochement CJ-1 résolu (manoeuvre_verifiee) |
| 02/09/2019 11:50:00 | manoeuvres | ana1 | vérification de PL-AEOLUS : conforme (écart 0 m) |
| 02/09/2019 11:50:00 | manoeuvres | systeme | plan PL-AEOLUS vérifié conforme |
| 02/09/2019 11:51:40 | coordination | systeme | CJ-1 close (resolu, depuis arretee) |
| 02/09/2019 11:51:40 | coordination | systeme | verrou CJ-1 clos : décision DS-CJ-1-1 soldée |
| 02/09/2019 13:00:00 | escalade | systeme | veille CJ-1 close (resolu) |
| 02/09/2019 13:00:00 | operations | systeme | gel d'urgence levé pour CJ-1 (resolu) |
| 02/09/2019 18:00:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-BRV\|execution : escalade refusée (Ferme) |
| 02/09/2019 18:00:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-BRV\|execution échu sans accusé |
| 02/09/2019 18:00:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-SPACEX\|execution : escalade refusée (Ferme) |
| 02/09/2019 18:00:00 | manoeuvres | systeme | avis PL-AEOLUS\|ORG-SPACEX\|execution échu sans accusé |
