import React, { createContext, useContext, useState, useCallback } from "react";

type ConfirmFn = (message: string) => Promise<boolean>;

const ConfirmContext = createContext<ConfirmFn>(async () => false);

export const useConfirm = () => useContext(ConfirmContext);

interface ConfirmState {
  open: boolean;
  message: string;
  resolve: ((value: boolean) => void) | null;
}

export const ConfirmProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [state, setState] = useState<ConfirmState>({ open: false, message: "", resolve: null });

  const confirm = useCallback((message: string): Promise<boolean> => {
    return new Promise(resolve => {
      setState({ open: true, message, resolve });
    });
  }, []);

  const settle = (value: boolean) => {
    setState(prev => ({ ...prev, open: false }));
    state.resolve?.(value);
  };

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      {state.open && (
        <div
          id="confirm-dialog-overlay"
          className="wardian-dialog-overlay fixed inset-0 z-[11000] flex items-center justify-center"
          onClick={() => settle(false)}
          onKeyDown={(event) => {
            if (event.key === "Escape") settle(false);
          }}
        >
          <div
            id="confirm-dialog-panel"
            role="dialog"
            aria-modal="true"
            aria-label="Confirm action"
            className="wardian-dialog-panel wardian-dialog-panel--compact relative mx-4 w-full p-6"
            onClick={e => e.stopPropagation()}
          >
            <p className="text-sm text-primary mb-6 leading-relaxed">{state.message}</p>
            <div className="flex gap-2 justify-end">
              <button
                id="confirm-dialog-cancel"
                onClick={() => settle(false)}
                autoFocus
                className="wardian-button wardian-button--secondary"
              >
                Cancel
              </button>
              <button
                id="confirm-dialog-confirm"
                onClick={() => settle(true)}
                className="wardian-button wardian-button--danger"
              >
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}
    </ConfirmContext.Provider>
  );
};
