import { Component, type ErrorInfo, type ReactNode } from "react";

/**
 * Why this exists: React has no error recovery. Without a boundary, one throw
 * during render unmounts the whole tree and the window stays blank until the
 * app is restarted by hand — and with no `window.onerror` handler either, the
 * failure is never reported. The boundary turns that dead frame into a
 * recoverable one.
 *
 * Reload is a deliberate manual action, not an automatic one: a deterministic
 * crash would otherwise become a reload loop.
 */
export class ErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Kept as console output on purpose: this is the only trace of the crash,
    // and it has to survive whatever took the UI down.
    console.error("MonoCode crashed:", error, info.componentStack);
  }

  private reset = () => {
    this.setState({ error: null });
    window.location.reload();
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        role="alert"
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: "16px",
          height: "100vh",
          padding: "32px",
          boxSizing: "border-box",
          fontFamily:
            "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
          color: "var(--fg, #e6e6e6)",
          background: "var(--bg, #161616)",
          textAlign: "center",
        }}
      >
        <h1 style={{ margin: 0, fontSize: "20px", fontWeight: 600 }}>
          Something went wrong
        </h1>
        <p style={{ margin: 0, maxWidth: "520px", opacity: 0.75 }}>
          MonoCode hit an unexpected error and stopped drawing. Your work is
          saved; reloading usually clears it.
        </p>
        <details style={{ maxWidth: "640px", textAlign: "left" }}>
          <summary style={{ cursor: "pointer", opacity: 0.75 }}>
            Error details
          </summary>
          <pre
            style={{
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              fontSize: "12px",
              opacity: 0.8,
              marginTop: "12px",
            }}
          >
            {error.message}
          </pre>
        </details>
        <button
          type="button"
          onClick={this.reset}
          style={{
            padding: "8px 20px",
            fontSize: "14px",
            fontWeight: 500,
            cursor: "pointer",
            borderRadius: "8px",
            border: "1px solid var(--border, #3a3a3a)",
            color: "inherit",
            background: "var(--button-bg, #262626)",
          }}
        >
          Reload
        </button>
      </div>
    );
  }
}
