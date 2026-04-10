(* ============================================================== *)
(*  Soma_CTL.v                                                      *)
(*                                                                  *)
(*  Mechanized soundness for the depth-bound fix in                 *)
(*    compiler/src/checker/temporal.rs::Property::Eventually        *)
(*                                                                  *)
(*  THEOREM proved here (no axioms, no Admitted):                   *)
(*                                                                  *)
(*    acyclic_walk_bounded:                                         *)
(*      In any finite graph G, every walk that visits no state      *)
(*      twice has length at most |states(G)|.                       *)
(*                                                                  *)
(*  This is the central fact that justifies the fix in the Soma     *)
(*  model checker:                                                  *)
(*                                                                  *)
(*    let bound = reachable.len() + 1                               *)
(*                                                                  *)
(*  Pre-fix the bound was a hard-coded `50`, which produced false   *)
(*  positives on liveness properties for state machines with more   *)
(*  than 50 reachable states. Post-fix, by `acyclic_walk_bounded`,  *)
(*  the bound is sufficient: any acyclic counter-example walk fits  *)
(*  within `|reachable(G)| + 1` steps.                              *)
(*                                                                  *)
(*  Compiled with: Rocq Prover 9.1.1                                *)
(*    coqc docs/rigor/coq/Soma_CTL.v                                *)
(* ============================================================== *)

From Stdlib Require Import List Arith Bool Lia.
Import ListNotations.

(* ============================================================== *)
(* §1.  Abstract state machines                                     *)
(* ============================================================== *)

Section Abstract.

(* The state name set is opaque — any type. Soma's runtime uses
   String, but nothing in the proof depends on that. *)
Variable State : Type.

(* A graph is a list of states, a list of directed edges, and an
   initial state. We do not bake `init ∈ states` into the type;
   the proofs that need it carry it as a hypothesis. *)
Record Graph : Type := mkGraph {
  states : list State;
  edges  : list (State * State);
  init   : State
}.

(* successor list of a state, computed from the edge list. The
   `eq_dec` is supplied per-call so we can reuse Graph for any
   State type. *)
Definition succ
  (eq_dec : forall a b : State, {a = b} + {a <> b})
  (G : Graph) (s : State) : list State :=
  map snd
    (filter
       (fun e => if eq_dec (fst e) s then true else false)
       G.(edges)).

(* ============================================================== *)
(* §2.  Walks                                                       *)
(* ============================================================== *)

(* `Walk eq G s t w` is the proposition: w is a list of states
   starting at s, ending at t, where every consecutive pair is
   an edge of G, and every state in w is in G's state set. *)
Inductive Walk
  (eq_dec : forall a b : State, {a = b} + {a <> b})
  (G : Graph) : State -> State -> list State -> Prop :=
| walk_refl :
    forall s,
      In s G.(states) ->
      Walk eq_dec G s s [s]
| walk_step :
    forall s s' t rest,
      In s G.(states) ->
      In s' (succ eq_dec G s) ->
      Walk eq_dec G s' t rest ->
      Walk eq_dec G s t (s :: rest).

(* Every state in a walk is in the graph's state set. *)
Lemma walk_states_subset :
  forall eq_dec G s t w,
    Walk eq_dec G s t w ->
    forall x, In x w -> In x G.(states).
Proof.
  intros eq_dec G s t w H.
  induction H as [s Hin | s s' t rest HinS Hsucc Hwalk IH]; intros x Hx.
  - simpl in Hx. destruct Hx as [Heq | []]. subst. exact Hin.
  - simpl in Hx. destruct Hx as [Heq | Hxrest].
    + subst. exact HinS.
    + apply IH. exact Hxrest.
Qed.

(* A walk is non-empty. *)
Lemma walk_nonempty :
  forall eq_dec G s t w,
    Walk eq_dec G s t w -> w <> [].
Proof.
  intros eq_dec G s t w H. inversion H; intros Hcontra; discriminate.
Qed.

(* ============================================================== *)
(* §3.  The pigeonhole lemma — the heart of the proof               *)
(* ============================================================== *)

(* The central fact: in any walk through a finite graph, if no state
   is visited twice, then the walk has at most as many states as the
   graph. *)
Theorem acyclic_walk_bounded :
  forall eq_dec G s t w,
    Walk eq_dec G s t w ->
    NoDup w ->
    length w <= length G.(states).
Proof.
  intros eq_dec G s t w Hwalk Hnd.
  apply NoDup_incl_length.
  - exact Hnd.
  - intros x Hx. eapply walk_states_subset; eauto.
Qed.

(* Contrapositive: any walk strictly longer than |states| must
   revisit some state. *)
Corollary walk_too_long_must_repeat :
  forall eq_dec G s t w,
    Walk eq_dec G s t w ->
    length w > length G.(states) ->
    ~ NoDup w.
Proof.
  intros eq_dec G s t w Hwalk Hlen Hnd.
  apply acyclic_walk_bounded in Hwalk; auto. lia.
Qed.

(* The exact form the Rust fix needs: any acyclic walk fits within
   `|states| + 1`, so a search bounded at that depth never prematurely
   truncates an acyclic counter-example. *)
Theorem bound_at_least_states_is_sufficient :
  forall eq_dec G s t w,
    Walk eq_dec G s t w ->
    NoDup w ->
    length w <= length G.(states) + 1.
Proof.
  intros eq_dec G s t w Hwalk Hnd.
  apply acyclic_walk_bounded in Hwalk; auto. lia.
Qed.

End Abstract.

(* ============================================================== *)
(* §4.  Concrete instantiation: a 4-state linear chain              *)
(* ============================================================== *)

(* We instantiate the abstract development at `nat` and exhibit a
   real walk in a real graph. This proves end-to-end that:
     (a) the definitions are not vacuous
     (b) the pigeonhole bound holds with equality (length = |states|)
     (c) the central theorem applies to a non-trivial walk
*)

Definition nat_eq_dec := PeanoNat.Nat.eq_dec.

Definition g4 : Graph nat :=
  mkGraph nat
    [0; 1; 2; 3]
    [(0,1); (1,2); (2,3)]
    0.

(* Successor checks. *)
Example succ_g4_0 : succ nat nat_eq_dec g4 0 = [1].
Proof. reflexivity. Qed.

Example succ_g4_1 : succ nat nat_eq_dec g4 1 = [2].
Proof. reflexivity. Qed.

Example succ_g4_2 : succ nat nat_eq_dec g4 2 = [3].
Proof. reflexivity. Qed.

Example succ_g4_3 : succ nat nat_eq_dec g4 3 = [].
Proof. reflexivity. Qed.

(* Build the walk 0 → 1 → 2 → 3 in g4. *)
Example walk_g4 : Walk nat nat_eq_dec g4 0 3 [0; 1; 2; 3].
Proof.
  apply walk_step with (s' := 1).
  - simpl. left. reflexivity.
  - rewrite succ_g4_0. simpl. left. reflexivity.
  - apply walk_step with (s' := 2).
    + simpl. right. left. reflexivity.
    + rewrite succ_g4_1. simpl. left. reflexivity.
    + apply walk_step with (s' := 3).
      * simpl. right. right. left. reflexivity.
      * rewrite succ_g4_2. simpl. left. reflexivity.
      * apply walk_refl. simpl. right. right. right. left. reflexivity.
Qed.

(* The walk is acyclic (no duplicates). *)
Example walk_g4_acyclic : NoDup [0; 1; 2; 3].
Proof.
  repeat (constructor; [intro H; simpl in H; lia | ]).
  constructor.
Qed.

(* The central theorem applies, giving the tight bound 4 ≤ 4. *)
Example walk_g4_bounded :
  length [0; 1; 2; 3] <= length (states nat g4).
Proof.
  apply acyclic_walk_bounded with (eq_dec := nat_eq_dec) (s := 0) (t := 3).
  - exact walk_g4.
  - exact walk_g4_acyclic.
Qed.

(* The "bound + 1" form used by the soundness fix. *)
Example walk_g4_bounded_plus_one :
  length [0; 1; 2; 3] <= length (states nat g4) + 1.
Proof.
  apply bound_at_least_states_is_sufficient
    with (eq_dec := nat_eq_dec) (s := 0) (t := 3).
  - exact walk_g4.
  - exact walk_g4_acyclic.
Qed.

(* In g4, ANY walk of length 5 must repeat a state. This is the
   pigeonhole fact in concrete form: 5 elements into 4 holes. *)
Example any_5_walk_in_g4_must_repeat :
  forall s t w,
    Walk nat nat_eq_dec g4 s t w ->
    length w = 5 ->
    ~ NoDup w.
Proof.
  intros s t w Hwalk Hlen Hnd.
  pose proof
    (acyclic_walk_bounded nat nat_eq_dec g4 s t w Hwalk Hnd) as Hbound.
  simpl in Hbound. lia.
Qed.

(* ============================================================== *)
(* §5.  Connection to the Soma model checker fix                    *)
(* ============================================================== *)

(*
The Rust fix in compiler/src/checker/temporal.rs (line ~301):

    let bound = reachable.len() + 1;
    let counter = graph.find_path_avoiding(&graph.init, pred, bound);

PRE-FIX: bound was hard-coded at 50.
POST-FIX: bound is `length(reachable) + 1`.

The mechanized correspondence:
  - Rust's `reachable.len()` ↔ `length (states G)` (in the worst
    case `reachable = states`, which is the bound we use).
  - Rust's `path` (a Vec<String>) ↔ Coq's `list State`.
  - Rust's `visited.contains(current)` cycle detection ↔ Coq's
    `NoDup` predicate over the path.
  - Rust's "DFS gives up at depth bound" ↔ "the abstract walk has
    length > bound", which by `walk_too_long_must_repeat` means the
    walk MUST contain a duplicate, which the runtime DFS catches via
    its `visited` set as a cycle counter-example.

By `bound_at_least_states_is_sufficient`, with bound ≥ length states,
the DFS cannot prematurely give up on an acyclic counter-example.
The 60-state chain regression test
  compiler/tests/rigor_eventually_long_chain.rs
exercises this concretely: pre-fix the verifier reported PASSING,
post-fix it correctly reports a 61-step counter-example.

WHAT THIS FILE PROVES MECHANICALLY:

  1. acyclic_walk_bounded
     ∀ eq_dec G s t w, Walk eq_dec G s t w →
                       NoDup w → length w ≤ length (states G).

  2. walk_too_long_must_repeat
     ∀ eq_dec G s t w, Walk eq_dec G s t w →
                       length w > length (states G) → ¬ NoDup w.

  3. bound_at_least_states_is_sufficient
     ∀ eq_dec G s t w, Walk eq_dec G s t w →
                       NoDup w → length w ≤ length (states G) + 1.

  4. Concrete witness on g4: a 4-state acyclic walk hitting the
     tight bound, plus the pigeonhole conclusion that any 5-step
     walk in g4 must repeat.

WHAT THIS FILE DOES NOT PROVE (tracked in docs/rigor/README.md):

  - The full operational correspondence between this abstract Graph
    and the Soma cell calculus (i.e. that the abstract Graph is a
    *correct abstraction* of every Soma program's reachable state
    space). This requires defining Soma's reduction relation in Coq.
  - Cyclic-counter-example soundness: the runtime DFS reports cycles
    via its `visited` set; we have not formalized that branch here.
    The pigeonhole bound covers the acyclic case where the bug lived.
  - Backend equivalence: bytecode VM and [native] codegen each need
    their own simulation theorems, separate from this file.

The single sentence:

  The depth bound `length(reachable) + 1` in temporal.rs is sufficient
  because, by `acyclic_walk_bounded`, every acyclic walk through a
  finite graph of n states has length ≤ n; mechanically verified
  in this file against Rocq Prover 9.1.1, with no axioms and no
  Admitted goals.
*)
