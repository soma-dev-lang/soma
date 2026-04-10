(* ============================================================== *)
(*  Soma_Isolation.v                                                *)
(*                                                                  *)
(*  The think-isolation theorem: CTL safety properties hold          *)
(*  regardless of what the LLM oracle returns.                       *)
(*                                                                  *)
(*  KEY INSIGHT: if every transition() call in a cell uses a         *)
(*  literal target (checked by the V1.3 refinement checker), then    *)
(*  the LLM can influence WHICH path through G(M) the execution      *)
(*  takes, but it CANNOT take the execution OUTSIDE G(M). Since      *)
(*  the model checker proves properties for ALL paths in G(M),       *)
(*  the specific path chosen by the oracle is irrelevant for safety. *)
(*                                                                  *)
(*  This is a corollary of:                                          *)
(*    - acyclic_walk_bounded (Soma_CTL.v): walks stay in states(G)   *)
(*    - walk_states_subset (Soma_CTL.v): every state on a walk is    *)
(*      in states(G)                                                 *)
(*                                                                  *)
(*  Status: 100% mechanized, no axioms, no Admitted.                 *)
(*  Compiled with: Rocq Prover 9.1.1                                 *)
(* ============================================================== *)

From Stdlib Require Import List Bool Arith.
Import ListNotations.

(* We reuse the Graph and Walk definitions from Soma_CTL.v.
   Rather than importing (which would require a build dependency),
   we re-state the minimal needed types and axiomatize the key
   property we need from Soma_CTL.v. This keeps the file self-
   contained and independently compilable. *)

Section Isolation.

Variable State : Type.
Variable State_eq_dec : forall a b : State, {a = b} + {a <> b}.

(* A finite graph: states, edges, initial. *)
Record Graph : Type := mkGraph {
  states : list State;
  edges  : list (State * State);
  init   : State
}.

(* Reachable states. *)
Definition reachable (G : Graph) : list State :=
  states G.  (* Conservative: every declared state is reachable. *)

(* A safety predicate: holds on a single state. *)
Definition SafetyPred := State -> bool.

(* "Safety holds on G" = the predicate holds on every state in G. *)
Definition safety_on_graph (G : Graph) (P : SafetyPred) : Prop :=
  forall s, In s (reachable G) -> P s = true.

(* A trace is a list of states the execution visited. *)
Definition Trace := list State.

(* "Trace stays in G" = every state in the trace is in reachable(G). *)
Definition trace_in_graph (G : Graph) (t : Trace) : Prop :=
  forall s, In s t -> In s (reachable G).

(* "Safety holds on trace" = the predicate holds on every state in the trace. *)
Definition safety_on_trace (P : SafetyPred) (t : Trace) : Prop :=
  forall s, In s t -> P s = true.

(* ============================================================== *)
(* THE THEOREM                                                      *)
(*                                                                  *)
(* If:                                                               *)
(*   1. The safety predicate holds on every state in G               *)
(*      (proven by soma verify for Always/Never/DeadlockFree/Mutex)  *)
(*   2. The execution trace stays in G                               *)
(*      (enforced by runtime fidelity + refinement + literal targets) *)
(* Then:                                                             *)
(*   The safety predicate holds on every state in the trace.         *)
(*                                                                  *)
(* This is independent of the oracle function — the trace could be   *)
(* ANY path through G, chosen by ANY oracle. Since safety holds for  *)
(* ALL states in G, it holds for any subset.                         *)
(* ============================================================== *)

Theorem think_isolation_safety :
  forall (G : Graph) (P : SafetyPred) (trace : Trace),
    safety_on_graph G P ->
    trace_in_graph G trace ->
    safety_on_trace P trace.
Proof.
  intros G P trace Hgraph Htrace.
  unfold safety_on_trace. intros s Hs.
  apply Hgraph.
  apply Htrace.
  exact Hs.
Qed.

(* ============================================================== *)
(* COROLLARIES                                                       *)
(* ============================================================== *)

(* Corollary 1: for any TWO oracles O1 and O2, if both produce
   traces that stay in G, then safety holds on both traces. *)
Corollary safety_oracle_independent :
  forall (G : Graph) (P : SafetyPred) (trace1 trace2 : Trace),
    safety_on_graph G P ->
    trace_in_graph G trace1 ->
    trace_in_graph G trace2 ->
    safety_on_trace P trace1 /\ safety_on_trace P trace2.
Proof.
  intros G P t1 t2 Hg H1 H2. split.
  - apply think_isolation_safety with G; assumption.
  - apply think_isolation_safety with G; assumption.
Qed.

(* Corollary 2: the empty trace trivially satisfies safety. *)
Corollary empty_trace_safe :
  forall (G : Graph) (P : SafetyPred),
    safety_on_graph G P ->
    safety_on_trace P [].
Proof.
  intros G P Hg. unfold safety_on_trace. intros s Hs. inversion Hs.
Qed.

(* Corollary 3: a single-state trace satisfies safety if the
   state is in G. *)
Corollary single_step_safe :
  forall (G : Graph) (P : SafetyPred) (s : State),
    safety_on_graph G P ->
    In s (reachable G) ->
    safety_on_trace P [s].
Proof.
  intros G P s Hg Hin. unfold safety_on_trace. intros s' Hs'.
  simpl in Hs'. destruct Hs' as [Heq | []]. subst.
  apply Hg. exact Hin.
Qed.

(* Corollary 4: concatenation of two safe traces is safe. *)
Corollary concat_safe :
  forall (P : SafetyPred) (t1 t2 : Trace),
    safety_on_trace P t1 ->
    safety_on_trace P t2 ->
    safety_on_trace P (t1 ++ t2).
Proof.
  intros P t1 t2 H1 H2. unfold safety_on_trace. intros s Hs.
  apply in_app_or in Hs. destruct Hs as [Hs | Hs].
  - apply H1. exact Hs.
  - apply H2. exact Hs.
Qed.

End Isolation.

(* ============================================================== *)
(* CONCRETE WITNESS (outside the Section so Graph is generalized)    *)
(* ============================================================== *)

(* A 3-state graph with predicate "not in {bad}". An adversarial
   oracle picks trace [s0, s1, s2] — safety still holds because
   bad is not in the graph's state set. *)

Definition nat_eq_dec := PeanoNat.Nat.eq_dec.

Definition g3 : Graph nat :=
  mkGraph nat [0; 1; 2] [(0,1); (1,2)] 0.

Definition not_bad : SafetyPred nat := fun s =>
  negb (Nat.eqb s 99).

Example g3_safety : safety_on_graph nat g3 not_bad.
Proof.
  unfold safety_on_graph, reachable. simpl. intros s Hs.
  destruct Hs as [H|[H|[H|[]]]]; subst; reflexivity.
Qed.

Example g3_trace_safe :
  safety_on_trace nat not_bad [0; 1; 2; 1; 0].
Proof.
  apply think_isolation_safety with (G := g3).
  - exact g3_safety.
  - unfold trace_in_graph, reachable. simpl.
    intros s Hs. destruct Hs as [H|[H|[H|[H|[H|[]]]]]]; subst;
    try (left; reflexivity); try (right; left; reflexivity);
    try (right; right; left; reflexivity).
Qed.

(* ============================================================== *)
(* WHAT THIS FILE PROVES                                             *)
(* ============================================================== *)

(*
PROVED MECHANICALLY (no axioms, no Admitted):

  1. think_isolation_safety
       If safety holds on G and the trace stays in G,
       then safety holds on the trace — regardless of the oracle.

  2. safety_oracle_independent
       Any two oracles producing traces in G both satisfy safety.

  3. empty_trace_safe, single_step_safe, concat_safe
       Structural corollaries.

  4. g3_trace_safe
       Concrete witness: an adversarial trace through a 3-state
       graph satisfies a safety predicate.

WHAT THIS FILE DOES NOT PROVE:

  - Liveness under adversarial oracle: that requires a fairness
    assumption (the oracle must eventually allow transitions).
    Safety is unconditional; liveness is conditional.
  - The connection from "all targets are literal" to "trace stays
    in G": this is by inspection of do_transition_for in the Rust
    runtime (Lemma 2.1 in SOUNDNESS.md) + the V1.3 refinement check
    (Lemma 2.2). Mechanizing the runtime guard in Coq requires
    formalizing the Soma reduction relation (V1.5 work).
  - Cross-cell isolation: if cell A passes think() output to cell B
    via delegate(), cell B's isolation depends on its own targets,
    not A's.

The single sentence:

  > If safety holds on the abstract graph (proven by soma verify)
    and the execution trace stays within that graph (enforced by
    runtime fidelity + literal targets from V1.3 refinement),
    then safety holds on the trace — for any LLM, any prompt,
    any response, any hallucination.
*)
