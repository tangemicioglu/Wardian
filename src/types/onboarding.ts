export type GuidedTourState = "unseen" | "in_progress" | "skipped" | "completed";

export interface OnboardingHintsState {
  dismissed_hint_ids: string[];
  contextual_tips_enabled: boolean;
  guided_tour_state?: GuidedTourState;
}
