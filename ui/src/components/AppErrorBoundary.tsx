import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { children: ReactNode };
type State = { error: Error | null };

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Nuvio UI render failed", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="app-crash-screen" role="alert" aria-live="assertive">
        <img src="/nuvio-wordmark.png" alt="Nuvio" />
        <strong>This page could not be displayed</strong>
        <p>{this.state.error.message || "An unexpected interface error occurred."}</p>
        <button onClick={() => window.location.reload()}>Reload Nuvio</button>
      </main>
    );
  }
}
