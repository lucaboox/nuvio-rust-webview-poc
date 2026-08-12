import { useState, type FormEvent } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { AccountPayload } from "../bridge/types";

export function AuthGate({ backendConfigured, onAuthenticated }: { backendConfigured: boolean; onAuthenticated(payload: AccountPayload): void }) {
  const [isSignUp, setIsSignUp] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setMessage(null);
    try {
      const method = isSignUp ? "auth.signUp" : "auth.signIn";
      const payload = await invoke<AccountPayload>(method, { email, password });
      if (payload.auth.status === "authenticated") {
        onAuthenticated(payload);
      } else {
        setMessage(payload.warning ?? "Check your email, then return here to sign in.");
        setIsSignUp(false);
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Authentication failed");
    } finally {
      setBusy(false);
    }
  }

  async function continueAsGuest() {
    setBusy(true);
    setMessage(null);
    try {
      onAuthenticated(await invoke<AccountPayload>("auth.continueAnonymous"));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Could not start guest mode");
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="auth-screen">
      <div className="auth-atmosphere" />
      <section className="auth-panel">
        <img src="/nuvio-wordmark.png" alt="Nuvio" />
        <span className="auth-kicker">RUST DESKTOP PREVIEW</span>
        <h1>{isSignUp ? "Create your account" : "Welcome back"}</h1>
        <p>{isSignUp ? "Create a Nuvio account and keep your setup in sync." : "Sign in to load your real profiles and synced addons."}</p>

        <form onSubmit={submit}>
          <label>Email<input type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} placeholder="you@example.com" required /></label>
          <label>Password<input type="password" autoComplete={isSignUp ? "new-password" : "current-password"} minLength={6} value={password} onChange={(event) => setPassword(event.target.value)} placeholder="At least 6 characters" required /></label>
          <button className="auth-submit" disabled={busy || !backendConfigured}>{busy ? "Connecting…" : isSignUp ? "Create account" : "Sign in"}</button>
        </form>

        {!backendConfigured && <div className="auth-message error">Backend configuration is missing. Add the public Nuvio client values to <code>.env.local</code>.</div>}
        {message && <div className="auth-message">{message}</div>}

        <button className="auth-switch" disabled={busy} onClick={() => { setIsSignUp((value) => !value); setMessage(null); }}>
          {isSignUp ? "Already have an account? Sign in" : "New to Nuvio? Create an account"}
        </button>
        <div className="auth-divider"><span>or</span></div>
        <button className="guest-button" disabled={busy} onClick={continueAsGuest}>Continue without an account</button>
        <small>Your refresh session is protected by Windows Credential Manager. Sign out to remove it.</small>
      </section>
    </main>
  );
}
