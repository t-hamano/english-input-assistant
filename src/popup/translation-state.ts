export interface TranslationResult {
  translated: string;
  explanation: string;
  source_is_english: boolean;
}

type ViewState =
  | { type: "loading" }
  | { type: "result"; result: TranslationResult; complete: boolean; error?: string }
  | { type: "error"; message: string };

export interface TranslationState {
  requestId: number | null;
  latestRequestId: number;
  originalText: string;
  view: ViewState;
}

export type TranslationAction =
  | { type: "start"; request_id: number; text: string }
  | { type: "update"; request_id: number; result: TranslationResult; complete: boolean }
  | { type: "error"; request_id: number; message: string }
  | { type: "preflight-error"; message: string }
  | { type: "dismiss" }
  | { type: "retry" };

export const initialTranslationState: TranslationState = {
  requestId: null,
  latestRequestId: 0,
  originalText: "",
  view: { type: "loading" },
};

export function translationReducer(state: TranslationState, action: TranslationAction): TranslationState {
  switch (action.type) {
    case "start":
      if (action.request_id <= state.latestRequestId) return state;
      return {
        requestId: action.request_id,
        latestRequestId: action.request_id,
        originalText: action.text,
        view: { type: "loading" },
      };
    case "update":
      if (action.request_id !== state.requestId) return state;
      return { ...state, view: { type: "result", result: action.result, complete: action.complete } };
    case "error":
      if (action.request_id !== state.requestId) return state;
      return {
        ...state,
        view: state.view.type === "result"
          ? { ...state.view, complete: true, error: action.message }
          : { type: "error", message: action.message },
      };
    case "preflight-error":
      return { ...state, requestId: null, view: { type: "error", message: action.message } };
    case "dismiss":
      return { ...state, requestId: null };
    case "retry":
      return { ...state, requestId: null, view: { type: "loading" } };
  }
}
