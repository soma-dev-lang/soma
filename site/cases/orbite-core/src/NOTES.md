# orbite-core — notes de conception

Binaire : soma 3.2.4. Machine de mesure : Apple M5. Rédigé le 15/11/2026.

## Chiffres

| | lignes |
|---|---|
| squelette (`app.cell`, jusqu'à `CORPS`) | 613 |
| corps (`app.cell`, `flesh`) | 705 |
| **programme** (`app.cell`) | **1 318** |
| `soma.toml` | 13 |
| tests : `test_colreg.cell` / `test_core.cell` / `test_aeolus.cell` | 87 / 339 / 166 = 592 |
| **total** | **1 923** |

12 cells (dont 6 machines : Operateurs, Catalogue, Rapprochements, Verrou, Coordination, Plans),
78 signaux, tous remplis (`soma holes` : « no holes »).

- `verify app.cell` : **VERIFY OK**, `108 passed, 0 warnings, 0 failures` ; 0 ✗, 0 « not explored »,
  0 « open obligation ». **1,9 s.**
- `verify --strict app.cell` : **VERIFY OK**, `114 proven (✓), 1 note (·)`. **1,9 s.** La note : 
  `Coordination.clore` fait `transition(r, "close")` sans nommer la source (les 7 arêtes vers
  `close` sont dans son cadre) ; ce n'est pas un ⚠.
- **13 règles entre machines prouvées** sur le programme complet, corps compris (11 sur les
  arêtes des cadres, 2 dans `soma.toml`), chacune sur son cône de 2 machines (12 à 23 états
  produits explorés). Le reste des 114 ✓ : par machine, accessibilité, absence de blocage,
  vivacité, raffinement (28 gestionnaires), terminaison ; 23 preuves d'invariants mémoire (une par
  écrivain, aucune laissée à l'exécution) ; 2 contrats (`ensures` de `niveau_pour`).
- Tests : 31 + 165 + 48 = **244 assertions**, toutes vertes ; `check .` : 3 fichiers, 0 erreur, 0 avertissement.

## Ce qui est prouvé, ce qui est testé

| Propriété | Prouvé par `verify` (programme complet) | Tenu par les corps, testé |
|---|---|---|
| 1. une désignation, un plan approuvé | règles 1–6 (verrou ↔ coordination), règle 11 (approbation ⇒ verrou tenu) | `approuve_de` : au plus un plan par rapprochement ; rouvrir / relever refusés sous un plan approuvé ; `UneSeuleDesignation` |
| 2. sans propulsion ou hors service, jamais | règle 10 : un plan n'est approuvé que si son objet est `manoeuvrant` (= en service AVEC propulsion) | `Verrou.prendre` refuse un objet non manœuvrant ; `SansPropulsion`, `HorsService` |
| 3. habité seulement si seul capable | — (habité est une donnée, pas un état) | `Priorites.qui_manoeuvre` (fonction pure) + `Verrou.prendre` ; `Habite`, `test_colreg` |
| 4. criblage, opérateur actif | règle 12 (approuvé ⇒ criblé avant), règle 9 (opérateur `actif` à l'approbation) | règle 8 (pc < 1/1 000 000), limite d'action ; `CriblageEtOperateur` |
| 5. réponse ou escalade ; le muet ne bloque pas | règles 7, 8, 13 (pas de coordination ni de décision sans rapprochement de niveau 3 en coordination) | accord tacite, escalade 1 → 3, reprise, relève ; `AccordTacite`, `RepriseParOrbite`, `Releve` |
| 6. traçabilité | — | journal `[immutable]` (ajout seul, imposé par le runtime) ; chronologie du rejeu |

Pourquoi la propriété 2 n'est prouvée qu'à l'approbation : la désignation vit dans le verrou
(`designe_de`), réécrite à chaque pose (rouverture, relève) ; une règle `via` exige un lien écrit
une seule fois. Le lien `objet_de` d'un plan, lui, ne change jamais : c'est là que la règle porte.
Avoir fait de « sans propulsion » un ÉTAT du catalogue (`inerte`) plutôt qu'un champ permet à une
seule règle de couvrir « sans propulsion OU hors service ».

## Mutations (la preuve n'est pas vide)

Chaque garde dont dépend une règle, retirée d'une copie, fait échouer exactement sa règle :

| Garde retirée | Règle qui tombe | Durée de `verify` |
|---|---|---|
| `require Coordination.decidable(r)` (Verrou.prendre) | 1 et 2 | 4 min |
| `require Coordination.libere(r)` (Verrou.lever) | 3 | 4 min |
| `Verrou.prendre` dans `Coordination.arbitrer` | 5 | 4 min |
| `Verrou.lever` dans `Coordination.rouvrir` | 6 | 3 min |
| `Rapprochements.engager` dans `Coordination.ouvrir` | 7, 8 et 13 | 6 min |
| `require Operateurs.est_actif(op)` (Plans.approuver) | 9 | 3,5 min |
| `require Catalogue.manoeuvrant(o)` (Plans.approuver) | 10 | 3,5 min |
| `require Verrou.tenu(r)` (Plans.approuver) | 11 | 13,5 min |

Un programme juste se vérifie en 2 s ; une règle fausse coûte plusieurs minutes, parce que `verify`
recherche alors la règle sur le produit des six machines (le cône ne suffit plus) : 3 à 14 min.

## Décisions

1. **Une entrée, `app.cell`** : squelette d'abord (vérifié seul : 8 règles ✓ en 0,5 s, 3 `via` en
   obligation ouverte), puis les corps dans le même fichier.
2. **Identifiants partagés** : rapprochement, coordination et verrou ont le même id (`CJ-1`). Les
   règles 1–8 et 13 n'ont donc aucun lien. Seuls les plans (id propre) ont trois liens
   (`objet_de`, `rapprochement_de`, `operateur_de`), écrits une fois par `Plans.declarer` après
   `require <lien>.get(plan) == () else Existe`, jamais effacés.
3. **Une condition par arête** : l'approbation traverse deux états de passage
   (`crible → habilite → capable → approuve`, dans le même appel), comme `ManPlan.approuver` de la
   version complète. Toutes les gardes sont lues AVANT la première transition.
4. **Des lecteurs d'état d'une ligne** (`on tenu(r) { return get_status(r) == "held" }`) : c'est la
   forme que `verify` sait suivre d'une cell à l'autre pour restreindre les états.
5. **Orchestration en haut** : `Orbite` (signaler, vérifier, rouvrir) et `Horloge` (balayer)
   enchaînent les cells de domaine, qui n'appellent que « vers le bas ». Un seul retour, en lecture :
   `Verrou` lit `Coordination.decidable` / `libere` — obligatoire, car `Verrou.prendre` est aussi un
   point d'entrée à lui seul, et les règles 1–3 doivent tenir pour tout appel. Aucun cycle de
   gestionnaires (terminaison prouvée partout), chaînes d'au plus 5 appels.
6. **Accord tacite** : à l'échéance, si l'opérateur du proposé a accepté et que les autres se
   taisent, la décision est arrêtée (un refus envoie à l'arbitrage sur-le-champ, il n'y a donc
   jamais de refus à l'échéance). Si c'est l'opérateur du proposé qui se tait : pas d'accord tacite,
   manquement, escalade, puis reprise.
7. **Reprise** à `TCA − préavis(proposé) − 2 h` plutôt qu'à `TCA − marge` fixe : à TCA − 2 h un
   objet à propulsion faible (72 h) ne peut plus rien. Même forme que la relève.
8. **Relève** (règle 17b) à `TCA − préavis(privilégié) − 2 h`, seulement si la décision a été posée
   avant cette ouverture (le désigné a eu sa chance) et si le préavis du privilégié tient encore.
   Dans le rejeu Aeolus, Starlink 44 (72 h) ne tient plus son préavis : pas de relève possible,
   elle n'est jamais tentée avec succès.
9. **Règles de barre** dans l'ordre de la version complète : capable (en service, propulsé,
   opérateur actif, préavis tenu) ; seul capable ; rang de manœuvrabilité (règle 18, l'habité
   exempté) ; rattrapage (règle 13) entre égaux seulement ; dernier lancé (règle 15) ; id. En mer
   la règle 13 l'emporte sur la 18 ; ici, comme dans ORBITE, la manœuvrabilité passe d'abord.
10. **Approbation explicite** (`systeme` ou analyste), non enchaînée au criblage : le test
    `Releve` y gagne un désigné qui a un plan criblé sûr et ne le fait jamais approuver.
11. **Invariant de taille** de `en_cours` : prouvé seulement quand le `require` et l'écriture sont
    dans un gestionnaire à eux (`_inscrire`) ; dans `ouvrir`, entre d'autres appels, il restait ⚠.
12. **Pièges 3.2.x évités** : aucun `require !a || b` (les négations sont dans des `if` ou en
    `== false`) ; aucune liste locale homonyme d'un slot (`trouvees`, `orgs`, `muettes`, `faits`…) ;
    aucune constante lue dans une autre cell (`Priorites.preavis` est un signal). Les aides de test
    ne portent pas le nom d'un signal du programme (`tca_cj1`, `tca_simule`, `criblage`).

## Ce que le noyau ne couvre pas (par rapport à https://soma-lang.dev/cases/orbite/)

Passerelle HTTP, habilitations détaillées (rôles, portées, sessions, accès d'urgence), éphémérides
(donc pas de « donnée périmée » dans la propriété 4), observation (la vérification d'orbite est un
écart fourni par l'appelant), météo spatiale, lancements, rentrées, fin de vie, responsabilité et
dossiers de preuve, partage de données, conformité (notation, mesures), opérations, versions et
dérogations des règles de priorité, tour de contre-proposition, grappes multi-objets, échelons
au-delà de 3 (États, voie diplomatique), notifications aux opérateurs autres que le journal.
