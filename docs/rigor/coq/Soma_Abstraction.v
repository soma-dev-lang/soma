(* ============================================================== *)
(*  Soma_Abstraction.v                                              *)
(*                                                                  *)
(*  The abstraction theorem: if the runtime guards every transition  *)
(*  against the abstract graph G(M), then safety properties proven   *)
(*  on G transfer to real executions.                                *)
(*                                                                  *)
(*  Status: 100% mechanized, no axioms, no Admitted.                 *)
(*  Compiled with: Rocq Prover 9.1.1                                 *)
(* ============================================================== *)

From Stdlib Require Import List Bool.
Import ListNotations.

Section Abstraction.

Variable State : Type.
Variable State_eq_dec : forall a b : State, {a = b} + {a <> b}.

(* The abstract graph. *)
Record Graph : Type := mkGraph {
  graph_states : list State;
  graph_edges  : list (State * State);
  graph_init   : State
}.

(* A safety predicate on states. *)
Definition SafetyPred := State -> bool.

(* "Safety holds on a graph" = P is true on init AND on every edge target. *)
Definition safety_on_graph (G : Graph) (P : SafetyPred) : Prop :=
  P (graph_init G) = true /\
  forall s t, In (s, t) (graph_edges G) -> P t = true.

(* A step: either a transition to a new state, or an internal step. *)
Inductive Step : Type :=
| Trans : State -> Step     (* transition to this target *)
| Internal : Step.          (* no state change *)

(* "Faithful to G": every Trans target is reachable by some edge from
   some state in G. This is the runtime guard (do_transition_for). *)
Definition faithful (G : Graph) (step : Step) : Prop :=
  match step with
  | Trans t => exists s, In (s, t) (graph_edges G)
  | Internal => True
  end.

(* Apply a step to a state. *)
Definition apply (current : State) (step : Step) : State :=
  match step with
  | Trans t => t
  | Internal => current
  end.

(* Execute a sequence of steps. *)
Fixpoint run (init : State) (steps : list Step) : State :=
  match steps with
  | [] => init
  | s :: rest => run (apply init s) rest
  end.

(* The trace: all states visited. *)
Fixpoint trace (init : State) (steps : list Step) : list State :=
  init :: match steps with
  | [] => []
  | s :: rest => trace (apply init s) rest
  end.

(* ============================================================== *)
(* THE CORE LEMMA: every state in a faithful trace satisfies P.      *)
(* ============================================================== *)

Lemma trace_safety_step :
  forall G P current step,
    safety_on_graph G P ->
    P current = true ->
    faithful G step ->
    P (apply current step) = true.
Proof.
  intros G P current step [Hpi Hpe] Hcur Hf.
  destruct step; simpl.
  - (* Trans s: there exists some source state with an edge to s *)
    destruct Hf as [src Hedge].
    apply Hpe with src. exact Hedge.
  - (* Internal: state unchanged *)
    exact Hcur.
Qed.

Theorem trace_all_safe :
  forall G P init steps,
    safety_on_graph G P ->
    P init = true ->
    Forall (faithful G) steps ->
    forall s, In s (trace init steps) -> P s = true.
Proof.
  intros G P init steps Hsg Hinit Hf.
  revert init Hinit.
  induction steps as [| step rest IH]; intros init Hinit s Hs.
  - (* Empty: trace = [init] *)
    simpl in Hs. destruct Hs as [Heq | []]. subst. exact Hinit.
  - (* step :: rest *)
    simpl in Hs. destruct Hs as [Heq | Hrest].
    + (* s = init *)
      subst. exact Hinit.
    + (* s is in the rest of the trace *)
      inversion Hf as [| step' rest' Hstep Hrest_f]; subst.
      apply IH with (init := apply init step); auto.
      apply trace_safety_step with G; auto.
Qed.

(* ============================================================== *)
(* THE ABSTRACTION THEOREM                                           *)
(*                                                                  *)
(* If:                                                               *)
(*   1. Safety holds on the abstract graph G                         *)
(*   2. The initial execution state matches G's initial state        *)
(*   3. Every step is faithful to G                                  *)
(* Then: safety holds on every state in the execution trace.         *)
(* ============================================================== *)

Theorem abstraction_transfers_safety :
  forall G P steps,
    safety_on_graph G P ->
    Forall (faithful G) steps ->
    forall s, In s (trace (graph_init G) steps) -> P s = true.
Proof.
  intros G P steps Hsg Hf s Hs.
  apply trace_all_safe with G (graph_init G) steps; auto.
  destruct Hsg as [Hpi _]. exact Hpi.
Qed.

(* ============================================================== *)
(* CONCRETE WITNESS                                                  *)
(* ============================================================== *)

End Abstraction.

(* A 3-state graph: 0 →1, 1→2. Safety: "not state 99". *)
Definition g3 : Graph nat := mkGraph nat [0;1;2] [(0,1);(1,2)] 0.

Definition not99 : nat -> bool := fun s => negb (Nat.eqb s 99).

Example g3_safe : safety_on_graph nat g3 not99.
Proof.
  split.
  - reflexivity.
  - intros s t Hin. simpl in Hin.
    destruct Hin as [H|[H|[]]]; inversion H; subst; reflexivity.
Qed.

(* Execution: Trans 1, Internal, Trans 2. *)
Example g3_execution_safe :
  forall s, In s (trace nat 0 [Trans nat 1; Internal nat; Trans nat 2]) ->
    not99 s = true.
Proof.
  apply trace_all_safe with g3.
  - exact g3_safe.
  - reflexivity.
  - constructor.
    + (* Trans 1 is faithful: edge (0,1) exists *)
      simpl. exists 0. left. reflexivity.
    + constructor.
      * (* Internal is faithful: True *)
        simpl. exact I.
      * constructor.
        -- (* Trans 2 is faithful: edge (1,2) exists *)
           simpl. exists 1. right. left. reflexivity.
        -- constructor.
Qed.

(* ============================================================== *)
(* WHAT THIS FILE PROVES                                             *)
(* ============================================================== *)

(*
PROVED MECHANICALLY (no axioms, no Admitted):

  1. trace_safety_step
       One faithful step preserves safety.

  2. trace_all_safe
       A full faithful execution preserves safety on every visited state.

  3. abstraction_transfers_safety
       THE ABSTRACTION THEOREM: safety on G + faithful steps → safety
       on every state in the real execution trace. This is the formal
       bridge between "soma verify proves P on G(M)" and "P holds on
       every real execution."

  4. g3_execution_safe
       Concrete witness: a 3-step execution through a 3-state graph
       satisfies the safety predicate.

WHAT THIS FILE DOES NOT PROVE:

  - That do_transition_for implements `faithful`. This is by inspection
    of the Rust runtime (SOUNDNESS.md §2, Lemma 2.1).
  - Liveness transfer (requires fairness — see Soma_Isolation.v).
  - That the graph G(M) is built correctly from the AST (by inspection
    of StateMachineGraph::from_ast in temporal.rs).

The single sentence:

  > If the runtime guards every transition against the abstract graph
    (formalized as `faithful`), then safety properties proven by the
    model checker on the graph hold on every state visited during
    real execution — mechanically verified in Rocq 9.1.1, no axioms.
*)
