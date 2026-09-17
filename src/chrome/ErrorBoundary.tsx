import { Component, type ErrorInfo, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
  /** Short label shown in the fallback so the user knows which area failed. */
  label: string;
  onError?: (error: Error, info: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Isolates render crashes to one surface instead of blanking the whole app.
 * Wrap top-level areas (App, SessionPane, transcript, editor) so a single
 * throwing component shows a retryable fallback.
 */
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: unknown): ErrorBoundaryState {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(`[ErrorBoundary:${this.props.label}]`, error, info);
    this.props.onError?.(error, info);
  }

  private handleRetry = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        role="alert"
        className="flex h-full min-h-0 flex-col items-center justify-center gap-2 p-6 text-center"
      >
        <p className="font-sans text-[13px] font-medium text-content">
          Something went wrong in {this.props.label}
        </p>
        <p className="max-w-md truncate font-mono text-[12px] text-content/50">
          {error.message}
        </p>
        <button
          type="button"
          onClick={this.handleRetry}
          className="rounded-md bg-content/8 px-2.5 py-1.5 font-sans text-[12px] text-content/60 hover:bg-content/12 hover:text-content"
        >
          Retry
        </button>
      </div>
    );
  }
}
