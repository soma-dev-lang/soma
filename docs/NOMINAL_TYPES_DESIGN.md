# Types nominaux et unités — proposition

> **Le but de Soma : permettre aux agents de programmation de concevoir de
> grandes applications de manière solide.** Cette proposition est jugée sur
> cette ligne.
>
> Statut : **décidé le 2026-11-14** (§6). Phase 0 faite le 2026-11-14 (`compiler/tests/nominal.rs`) ; phases 1 et 2 à faire.
> Chaque affirmation sur le comportement actuel a été vérifiée avec
> `soma 3.1.0` (`compiler/target/release/soma`) ; les programmes cités tiennent
> dans ce fichier et se rejouent tels quels.

## TL;DR

La critique : « pas d'unités, pas de types nominaux (Lots, Centimes) ; un
type `Lots` distinct aurait rendu le bug du plafond impossible à compiler ».
Elle est juste, et c'est pire qu'elle ne le dit : aujourd'hui, `soma verify
--strict` **prouve** un invariant écrit dans la mauvaise unité, et les deux
façons « naturelles » qu'un agent essaierait (`q: Lots` sans déclaration,
`q: Int<lots>`) sont acceptées en silence et vérifient *moins* qu'un `Int`.

Recommandation : **un type nominal effacé**, `cell type Lots = Int`.
Vérifié par la passe de types (`checker/types.rs`), puis effacé vers `Int`
avant tout le reste : le prouveur, l'interpréteur, `[native]`, le stockage et
le JSON voient un `Int`, donc rien de ce qui est prouvé aujourd'hui ne bouge
et les données existantes restent valides. Opt-in : un programme sans
déclaration garde son verdict. Ensuite (phase 2) un prédicat `where` sur le
type nominal, compilé en `requires` / invariants que `verify` sait déjà prouver.
Les unités dimensionnelles (`Int<cents/lot>`) : non, pas maintenant.

## 1. Le problème

### 1.1 Le programme minimal

```soma
cell Risk {
    face {
        signal order(sym: String, lots: Int, price_cents: Int) -> Int
    }
    const MAX_EXPOSURE = 5_000_000            // centimes
    memory {
        position: Map<String, Int> [persistent]   // lots
        invariant position >= 0 && position <= MAX_EXPOSURE
    }
    on order(sym: String, lots: Int, price_cents: Int) {
        require lots > 0 else BadQty
        let held = position.get(sym) ?? 0
        require held + lots <= MAX_EXPOSURE else OverCap   // lots comparés à des centimes
        position.set(sym, held + lots)
        return held + lots * price_cents                   // lots + centimes
    }
}
```

```
$ soma check bug.cell
✓ All checks passed.
$ soma verify --strict bug.cell
  ✓ invariant position >= 0 && position <= 5000000 — writer 'order' proven by induction (writes held + lots)
VERIFY OK
```

Deux bugs de la même classe — une somme de lots et de centimes, un plafond
en centimes appliqué à des lots — et la preuve est verte. C'est le point le
plus grave pour Soma : un `✓` est une pièce qu'un agent cite comme preuve ;
ici elle porte sur la mauvaise quantité. Le prouveur raisonne juste sur
des `Int` ; c'est le langage qui ne lui dit pas que ces `Int` ne sont pas
interchangeables.

### 1.2 Ce qui existe aujourd'hui (vérifié)

Le typage statique est `checker/types.rs` (3.0) : inférence locale, `Ty` =
`Int | Float | Num | Str | Bool | List | Map | Record | Variant | Opt | Unknown…`,
**unilatéral** — une erreur seulement quand les deux côtés sont connus et que
l'interpréteur lèverait `type` à chaque exécution (`Unknown` passe partout).
Pas d'alias, pas de newtype ; `let` n'a pas d'annotation (`let x: Int = 3` est
une erreur de syntaxe) ; `const` n'accepte qu'un littéral.

| On écrit | `check` | `run` | `verify` |
|---|---|---|---|
| `lots + price_cents` (deux `Int`) | passe | passe | — |
| plafond en centimes sur des lots (§1.1) | passe | passe | `--strict` : ✓ |
| `q: Lots`, `Lots` déclaré nulle part | **passe** (lu comme `Any`) | accepte `"hello"`, échoue plus loin | perd « c'est un Int » |
| `q: Integer` | E0046 « did you mean `Int`? » | — | — |
| `q: Int<lots>` | **passe**, et éteint les contrôles : `s(q)` vers `s(x: String)` accepté (refusé si `q: Int`) | contrôlé comme `Int` | `require q < 3; set(k, q + 1)` sous `<= 3` : ⚠ runtime-checked (✓ avec `q: Int`) |
| `cash.set(k, "x")`, `cash: Map<String, Int>` | passe | refusé, kind `type` | — |
| `invariant pos <= "cap"`, `requires q <= "big"` | passe (les clauses ne sont pas typées) | `requires` : violée | propose `require q <= "cap"` |
| face `-> Int`, handler `return n / 2` | passe (`Num`) | refusé : `returned Float 1.5` | — |

**Le contournement actuel : un record à un champ.**
`cell type Lots { variants { Lots { n: Int } } }`, et de même `Cents`.

| On écrit | `check` | `run` | `verify` |
|---|---|---|---|
| `take(3)` pour `take(q: Lots)` | E0105 | — | — |
| `take(Cents { c: 3 })` | **passe** (records = même classe `MapLike`) | refusé, kind `type` | — |
| `a == b` (Lots contre Cents) | passe | `false`, en silence | — |
| `a.n + b.c` | passe | passe | — |
| `a + b` (deux Lots) | E0109, fix proposé « convert one side (to_int…) » — faux pour un record | — | — |
| slot `Map<String, Lots>`, `invariant pos.n <= 50`, écriture après `require q <= 50` | — | — | ⚠ runtime-checked (le prouveur ne suit pas les champs) |
| `on f(q: Lots) [native]` | E0038 (paramètres natifs : Int, Float, Bool, String) | — | — |
| stockage | — | `{"_type":"Lots","_variant":"Lots","n":2}` | — |

Le record protège à l'exécution et seulement là ; il coûte la preuve, le
natif, la lisibilité et le format stocké. Un agent ne l'utilisera pas.

**Pourquoi un « newtype » naïf casserait le prouveur.** `invariants.rs` et
`verify.rs` reconnaissent le *texte* `Int` d'un paramètre ou d'un retour
(`invariants.rs` : 1784, 1851, 2079, 3617, 3696, 3958, 4017 ;
`verify.rs:1643`) pour savoir qu'une valeur est entière (`q < 3` ⇒ `q + 1 <= 3`)
ou sans NaN. La ligne `Int<lots>` du tableau le montre : tout nom autre que
`Int` fait perdre des preuves. D'où l'effacement (§2.A).

### 1.3 Le cas réel

`eom-es/app.cell` (le seul programme du dépôt qui manipule un plafond de
lots, non suivi par git) lit `intended` et `cap` dans un `scenario: Map`
(l. 164-167). Une valeur lue dans une `Map` est `Unknown` : **aucune** option
ci-dessous ne la verrait sans que le scénario devienne un record typé
(`cell type Scenario { variants { Scenario { intended: Lots, cap: Lots, … } } }`).
Les types nominaux protègent là où les types circulent ; il faudra le dire
dans la doc (question ouverte 8).

## 2. Les options

Les mêmes questions pour chacune : syntaxe, ce que `check` refuse, le message
(qui nomme le correctif, en anglais comme tous les diagnostics), `verify`,
frontières JSON/HTTP, `[native]`, données persistantes, squelettes.

### A. Type nominal effacé : `cell type Lots = Int`

```soma
cell type Lots  = Int
cell type Cents = Int
cell type Sym   = String            // aussi pour les identifiants

cell Risk {
    const MAX_LOTS = Lots(50)       // nouveau : un littéral enveloppé
    memory {
        position: Map<Sym, Lots> [persistent]
        invariant position >= 0 && position <= MAX_LOTS
    }
    on order(sym: Sym, lots: Lots, price: Cents) {
        let held = position.get(sym) ?? 0                    // Lots (le littéral s'adapte)
        require held + lots <= MAX_LOTS else OverCap
        position.set(sym, held + lots)
        return Cents(to_int(lots) * to_int(price))           // conversion écrite
    }
}
```

Bases admises : `Int`, `Float`, `String`. Construire : `Lots(n)` (la syntaxe
du constructeur de variante tuple). Sortir : `to_int(q)` / `to_float(q)` /
`to_string(q)`, builtins existants, identité à l'exécution.

Règles de la passe de types (`L`, `M` nominaux distincts, `n` un `Int` simple) :

| Expression | Résultat |
|---|---|
| `L + L`, `L - L`, `-L`, `abs(L)`, `L % n`, `idiv(L, n)`, `min`/`max`/`sum` sur `List<L>` | `L` |
| `L * n`, `n * L` (mise à l'échelle) | `L` |
| `L / L` | nombre sans type (un ratio) |
| `L / n` | nombre sans type (`/` est exact) ; `idiv` garde `L` |
| `L op littéral`, `L cmp littéral`, `x ?? 0` | le littéral prend le type `L` |
| `let t = 0` puis `t += q` | `t` devient `L` |
| `L` contre `L` : `< <= == …` | `Bool` |
| `L + M`, `L cmp M`, `L == M`, `L * L`, `L * M` | **refusé** |
| `L + n`, `L cmp n` (variable `Int`) | **refusé** — dire ce que compte `n` |
| `L` passé où `Int` est déclaré, ou l'inverse | **refusé** (sauf littéral) |
| `Unknown` (un `Any`, un champ de `Map`) avec `L` | passe, comme aujourd'hui |

Messages :

```
error: `held + lots <= MAX_EXPOSURE` compares Lots with Cents — two nominal types never compare;
       if the cap counts lots, declare it so: `const MAX_LOTS = Lots(50)`;
       if a conversion is meant, write it: `Cents(to_int(lots) * to_int(price))`
error: `held + n` adds an Int to Lots — say what `n` counts: `Lots(n)` if it is lots, or `to_int(held) + n` to leave the type
error: `a * b` multiplies Lots by Lots, which is not Lots — scale by a plain number (`a * 2`), or leave the type: `to_int(a) * to_int(b)`
error: order(): argument 2 (`c`) is Cents but the handler declares `lots: Lots` — pass a Lots, or convert explicitly: `Lots(…)`
```

Nouveau code `nominal_mismatch`, ajouté en fin de `ERROR_KINDS`
(`checker/codes.rs`, table append-only), pour que `soma explain` l'enseigne.

- **Clauses.** La passe type aussi, quand un type nominal y figure : les
  invariants, `requires` / `ensures`, les gardes, les `require`. C'est là
  qu'est le plafond (§1.1). Aujourd'hui aucune clause n'est typée
  (`invariant pos <= "cap"` passe) ; les typer seulement en présence d'un
  nominal ne change aucun verdict existant.
- **`verify`.** Après `check`, `checker/nominal.rs` réécrit `Lots` → `Int`
  partout (paramètres, slots, retours, champs de records, `const`) et
  `Lots(x)` → `x` contrôlé. Le prouveur lit un programme ordinaire, avec les
  mêmes preuves : `invariant position <= MAX_LOTS` est ✓ comme avec `Int`.
  Même procédé que `flesh::attach` : « à partir de là, le programme est fait de
  `CellDef` ordinaires ». Le théorème du §1.1 devient une erreur de `check`
  au lieu d'un ✓.
- **JSON / HTTP.** Effacé : un `Lots` est un nombre nu. `soma serve` coerce un
  segment de chemin vers la base (`/order/ES/3`), un corps `{"lots": 3}` est
  contrôlé comme `Int`, `{"lots": "3"}` refusé comme aujourd'hui. `to_json`,
  `from_json` vers un record à champ `Lots`, `think_json(map("as", …))` : la
  forme montrée et contrôlée est celle de la base. Le type ne traverse pas le
  fil : la frontière fait ce qu'elle fait pour `Int`, ni plus ni moins.
- **`[native]`.** Après effacement, `on f(q: Lots) [native]` est un paramètre
  `Int` : accepté. La passe de types saute les corps natifs aujourd'hui
  (`types.rs`, « its own, stricter typing ») : les appels *vers* un natif sont
  contrôlés, l'intérieur non (question 6).
- **Données persistantes.** Rien à migrer. La table d'un slot s'appelle
  `Cell_slot` (`A_pos`) ; le type déclaré n'en fait pas partie et les valeurs
  restent des `Int`. Passer `Map<String, Int>` → `Map<String, Lots>` sur une
  base remplie relit les mêmes valeurs ; revenir en arrière aussi.
- **Squelettes.** Les frames ne changent pas. Le brief a déjà une ligne
  `types` (vu sur `soma skeleton app.cell Risk.order` : `types Lots = Lots { n: Int }`) :
  elle dirait `Lots = Int (nominal: build Lots(n), read to_int(q))`. Le lock
  « suit les types que les signatures nomment » : changer `Lots = Int` en
  `Lots = Float` devient un `--amend`. C'est le bon endroit : dans un
  squelette, l'unité *est* une décision de conception, et un brief qui dit
  `lots: Lots, price: Cents` en dit plus que `Int, Int`.
- **Limite.** Effacé veut dire : pas de contrôle d'unité à l'exécution. Une
  valeur passée par `Any` ou par une `Map` perd son type en silence. C'est le
  prix de « aucune donnée à migrer, aucune preuve perdue ».

### B. Unités dimensionnelles : `Int<lots>`, `Int<cents/lot>`

```soma
on order(lots: Int<lots>, price: Int<cents/lot>) {
    let notional = lots * price              // Int<cents>, inféré
    require notional <= MAX_NOTIONAL else OverCap
}
```

`+ - < ==` exigent la même dimension ; `* /` additionnent ou soustraient les
exposants ; les littéraux s'adaptent.

- **Refuse** tout ce que refuse A, et accepte sans conversion écrite ce que A
  oblige à écrire (`lots * price` est des centimes).
- **Message** : `cannot add Int<lots> and Int<cents>` ; pour `*` le message
  doit afficher une dimension calculée (`Int<cents·lot⁻¹·…>`), et dans
  F# c'est là que les messages deviennent illisibles. Un agent corrige un
  message qui nomme deux types ; il tourne en rond sur une algèbre.
- **Ne couvre pas** les identifiants (`OrderId` contre `Sym` : `String` n'a pas
  d'unité), ni deux grandeurs de même dimension et de sens différent (lots ES
  contre lots MES).
- **verify / JSON / native / stockage / squelettes** : comme A (effacé).
- **Coût** : algèbre d'exposants et normalisation dans `types.rs`, règles pour
  `sqrt`, `pow`, `round`, `stdev`, `/` exact (`Int<cents> / Int<lot>` est un
  `Num<cents/lot>`), extension du parseur (`cents/lot` n'est pas un argument
  générique aujourd'hui). Environ deux fois A.
- Attention : `Int<lots>` **parse déjà** et passe `check` (§1.2). Choisir B
  donnerait un sens à une syntaxe aujourd'hui silencieuse ; ne pas la choisir
  oblige à la refuser.

### C. Raffinement : `Int where 0 <= value <= 50`

```soma
cell type Lots = Int where value >= 0 && value <= 50
```

Le prédicat devient un `requires` sur chaque paramètre de ce type, un
invariant sur chaque slot qui en contient, un `ensures` sur chaque retour.
Tout cela, `verify` sait déjà le prouver ou le dire runtime-checked
(« `⚠ … runtime-checked at 1 of 2 call sites: pay → debit(amount)` »).

- **Ne règle pas la classe de bug** : lots (≤ 50) contre plafond en centimes,
  `held + lots <= 5_000_000` reste vrai, donc prouvé. Un raffinement borne une
  valeur ; il ne dit pas *ce qu'elle compte*.
- Sur `Int` nu, c'est un `requires` écrit ailleurs. Sur un type nominal (A),
  la borne est écrite une fois et le prouveur l'applique partout : phase 2.
- JSON / HTTP : contrôlé à l'entrée (400 kind `requires`), comme une clause de
  face. Stockage : une donnée existante hors bornes n'est relue que si on la
  réécrit (invariant contrôlé à l'écriture) — à documenter.

### D. Ne rien changer au langage : une lint sur les suffixes

```
warning: `held_lots + cap_cents` mixes _lots with _cents — convert one side explicitly,
         or declare nominal types (`cell type Lots = Int`) to have it checked
```

Coût minime (`checker/habits.rs`), aucune syntaxe. Heuristique : dans le
corpus suivi (2 268 fichiers), 506 paramètres ou champs `Int` portent un nom
d'unité (`amount` 234, `qty` 78, `cents` 70…), rarement des deux côtés d'une
opération ; `eom-es` (`intended`, `cap`) passerait au travers. Jamais une
preuve ; au mieux un marchepied vers A.

### Comparaison

| | A nominal | B dimensions | C raffinement | D lint |
|---|---|---|---|---|
| bug §1.1 refusé par `check` | oui | oui | non | si suffixes |
| identifiants (`Sym` / `OrderId`) | oui | non | non | non |
| preuves existantes | intactes (effacé) | intactes | enrichies | intactes |
| données / JSON | inchangées | inchangées | entrée contrôlée | inchangées |
| `[native]` | oui (après effacement) | oui | prédicat hors natif | — |
| messages pour un agent | deux noms de types | algèbre | une clause | heuristique |
| taille | ~800 lignes | ~1 500 | ~400 sur A | ~100 |

## 3. Recommandation et migration

**A**, plus deux réparations des pièges actuels, puis **C** posé sur A.
B : non (on garde de la place : voir question 7). D : facultatif.

Aucun verdict existant ne change :

- **Phase 0 (3.2, avertissements).** Un nom de type déclaré nulle part et
  `Int<…>` / `String<…>` / `Float<…>` deviennent des avertissements en
  édition 2026, des erreurs en 2027 :
  ```
  warning: type `Lots` is declared nowhere, so it accepts any value (Any) — declare it: `cell type Lots = Int` (an error in edition 2027)
  warning: `Int<lots>` — Int takes no parameter, so this checks nothing and turns the Int checks off; declare a nominal type: `cell type Lots = Int`, then `lots: Lots`
  ```
  Corpus : aucun `Int<…>` ; un scan approximatif ne trouve que 2 noms non
  déclarés (`Integer`, déjà refusé, et un `T`). `soma fix --edition 2027`
  propose la déclaration.
- **Phase 1 (3.2).** `cell type X = Int | Float | String`, `X(v)`,
  `const C = X(literal)`. Opt-in : sans déclaration, la passe est celle
  d'aujourd'hui. `tools/verdicts.sh` doit afficher `0 changed`.
- **Adopter dans un programme existant** : déclarer `Lots`, retyper les
  paramètres et slots, laisser `check` lister les mélanges ; aucune donnée ni
  client HTTP ne change ; `soma skeleton --amend` si la structure est gelée.
  La doc (`llms.txt`, section « Sum types » → « Types ») ajoute un gotcha :
  une valeur lue dans une `Map` n'a pas de type ; un record la lui donne.
- **Phase 2.** `where` sur un type nominal (C).

## 4. Implémentation

| Fichier | Changement | Lignes |
|---|---|---|
| `parser/mod.rs` (~887-912) | après `cell type Nom`, accepter `= Base [where cond]` ; `const C = Nom(littéral)` (~328) | 60 |
| `ast/mod.rs` | `CellDef.nominal: Option<NominalDecl>` | 20 |
| `checker/types.rs` | `Ty::Nominal(nom)` et sa base ; `ty_of` ; table des opérateurs (`arith` ~660-715, `compare` ~717-760) ; littéraux qui s'adaptent ; `assignable` ; constructeur et `to_int`/`to_float`/`to_string` ; typage des clauses quand un nominal y figure | 350 |
| `checker/nominal.rs` (nouveau) | effacement après `check`, avant `verify`, `[native]`, l'interpréteur ; `describe`, `skeleton` et le lock lisent le programme non effacé | 200 |
| `checker/mod.rs` (~828-880) | phase 0 : nom non déclaré, `Int<…>` | 50 |
| `checker/native.rs:60` | résoudre le nominal avant de refuser le paramètre | 10 |
| `interpreter` | `Lots(x)` : identité après contrôle de la base (le chemin du constructeur de variante) | 40 |
| `commands/describe.rs`, `commands/skeleton.rs` (`types_named`), `checker/lock.rs` | afficher `Lots = Int (nominal)` ; empreinte | 60 |
| `checker/codes.rs`, gotchas, `llms.txt`, `soma explain` | `nominal_mismatch` | 60 |

`invariants.rs`, `verify.rs`, `product.rs`, le stockage, `serve` : **aucun
changement** — c'est l'intérêt de l'effacement. Environ 850 lignes de Rust et
400 de tests.

**Tests à écrire d'abord** (`compiler/tests/nominal.rs`, fixtures sous
`tests/fixtures/nominal/`) :

1. §1.1 réécrit avec `Lots` / `Cents` : `check` refuse, message exact, code.
2. **Monotonie de la preuve** : pour chaque programme du corpus qui a un
   `verify`, remplacer un paramètre `Int` par `Lots` (déclaré `= Int`) partout
   où il circule doit donner la *même* sortie de `verify` (même script que
   `tools/skeleton_monotone.py`).
3. `p/int.cell` (`require q < 3` ; `set(k, q + 1)` sous `invariant <= 3`) avec
   `q: Lots` : ✓ comme avec `Int` — le test qui échoue aujourd'hui avec `Int<lots>`.
4. Littéraux : `q <= 50`, `q + 1`, `?? 0`, `let t = 0; t += q`.
5. `L * L`, `L + M`, `L == M`, `L + n`, argument `Cents` vers `Lots`,
   appel inter-cellules (`Ledger.post(c)`) : refusés ; `L * n`, `idiv(L, n)`,
   `sum(List<L>)` : `L`.
6. Clauses : `invariant position <= MAX_EXPOSURE` (Lots contre Cents) refusé ;
   `invariant pos <= "cap"` sans nominal : toujours accepté (aucun verdict ne
   change).
7. Frontières : `soma serve` `/order/ES/3` et corps `{"lots": 3}` acceptés,
   `{"lots": "3"}` refusé ; `from_json` vers un record à champ `Lots`.
8. Stockage : remplir un slot `Map<String, Int>`, le retyper en `Lots`, relire.
9. `[native]` avec un paramètre `Lots` : compile, mêmes résultats que
   l'interprète.
10. Squelette : le brief montre le type ; changer la base casse le lock.
11. Phase 0 : nom non déclaré et `Int<lots>` avertis en 2026, refusés en 2027.
12. `tools/verdicts.sh` : 0 fichier ne change de verdict.

## 5. Questions ouvertes

1. **Littéraux.** Proposé : un littéral prend le type de l'autre côté
   (`q <= 50`). F# exige `50<lots>`. Plus sûr, plus lourd ; le bug visé
   oppose deux *variables*, pas une variable et un littéral.
2. **Mise à l'échelle.** `L * n` avec `n` variable `Int` : accepté (proposé,
   pour `lots * multiplier`) ou refusé ?
3. **Division.** `L / n` perd le type (proposé) ou le garde ?
4. **Effacé ou étiqueté à l'exécution ?** Proposé : effacé (aucune migration,
   aucune preuve perdue, aucun coût). Étiqueté rattraperait les valeurs passées
   par `Any`, au prix du format stocké et du JSON.
5. **Clauses typées pour tout le monde en 2027 ?** `invariant pos <= "cap"`
   passe aujourd'hui et `verify` propose `require q <= "cap"`.
6. **L'intérieur des corps `[native]`** : étendre `native.rs` à la règle
   nominale, ou laisser le natif « non typé en unités » ?
7. **Produits déclarés** (un B réduit) : `cell type Notional = Cents` et une
   règle `Lots * CentsPerLot -> Cents` déclarée, pour éviter `to_int(...)`
   dans le calcul du notionnel ?
8. **Les `Map`.** Un paramètre `scenario: Map` dont un champ est comparé à un
   `Lots` : avertir (« make Scenario a record to have its fields checked ») ou
   seulement documenter ?
9. **Nommage.** `cell type Lots = Int` (proposé : rien de nouveau à apprendre)
   ou `type Lots = Int` ?
10. **Portée.** Les noms de `cell type` sont globaux au programme, imports et
    paquets compris : deux paquets qui déclarent `Cents` différemment ?
11. **Code d'erreur.** Nouveau `nominal_mismatch`, ou réutiliser E0109
    (`type_mismatch`) ?

## 6. Décision (2026-11-14)

Antoine a délégué la décision. Retenu : **A** (type nominal effacé), précédé de
la **phase 0**, puis **C** sur A. **B** et **D** : non. Le critère est la
devise : un ✓ que l'agent cite doit porter sur la bonne quantité, et chaque
refus doit nommer la correction.

Ordre de livraison :

1. **Phase 0 seule d'abord** (petite, utile même si la suite tarde) : un nom
   de type déclaré nulle part et `Int<…>` / `Float<…>` / `String<…>` sont un
   avertissement en édition 2026, une erreur en 2027.
2. **Phase 1** : `cell type X = Int | Float | String`, `X(v)`, `const C = X(lit)`,
   règles de la passe de types, effacement avant `verify`. Les 12 tests du §4
   d'abord ; `tools/verdicts.sh` et la comparaison `verify` sur les 2 268
   programmes suivis : 0 changé.
3. **Phase 2** : `where` sur un type nominal.

Réponses aux questions du §5 :

| # | Décision | Pourquoi |
|---|---|---|
| 1 | Un littéral prend le type de l'autre côté (`q <= 50`, `?? 0`). | Le bug visé oppose deux variables. Exiger `Lots(50)` partout pousse l'agent vers `to_int(…)` pour se débarrasser du type, ce qui annule le bénéfice. |
| 2 | `L * n` avec `n` **variable** `Int` : **refusé** ; avec un littéral ou une `const` littérale : `L`. | `lots * price_cents` avec un prix non typé donnerait des « lots » faux : exactement la classe de bug. Le message dit `Lots(to_int(q) * n)` si `n` est un multiplicateur. À revoir sur données d'usage. |
| 3 | `L / x` → nombre sans type. `idiv(L, k)` garde `L` avec `k` littéral, refusé avec une variable (même règle que 2). | Cohérent avec 2 ; `/` est exact, un ratio n'a plus d'unité. |
| 4 | **Effacé.** | Aucune migration, aucune preuve perdue, JSON inchangé. La perte par `Any` est traitée par 8. |
| 5 | Clauses typées pour tous : avertissement en 2026, erreur en 2027. | `invariant pos <= "cap"` qui passe, et un `verify` qui propose `require q <= "cap"`, sont des faux ✓ en puissance. |
| 6 | Corps `[native]` : non typés en unités en phase 1 (les appels vers un natif le sont). | Le natif est déjà un sous-ensemble strict ; l'étendre plus tard si un cas réel le demande. |
| 7 | Pas de produits déclarés. Conversion écrite (`Cents(to_int(lots) * to_int(price))`). | C'est un B réduit ; on attend de voir si les conversions écrites gênent vraiment. |
| 8 | **Avertir** quand une valeur lue dans une `Map` (ou `Any`) rencontre un nominal dans une opération ou une comparaison : `make Scenario a record to have its fields checked`. Avertissement, jamais erreur. | C'est le cas réel (`eom-es`) ; le documenter seul ne l'aurait pas rattrapé. |
| 9 | `cell type Lots = Int`. | Rien de nouveau à apprendre ; même mot-clé que les autres types. |
| 10 | Noms globaux. Deux déclarations identiques fusionnent ; deux bases différentes pour un même nom sont une erreur au chargement, qui nomme les deux fichiers. | Un `Cents` qui changerait de base selon l'import serait un piège silencieux. |
| 11 | Nouveau code `nominal_mismatch`, ajouté en fin de table. | Une page `/errors/` et un `soma explain` à lui ; E0109 couvre déjà trop de cas différents. |

D (lint sur les suffixes) n'est pas retenu : heuristique, jamais une preuve,
et la phase 0 plus la décision 8 couvrent mieux le même besoin.
