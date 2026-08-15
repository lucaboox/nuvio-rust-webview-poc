import { Eye, EyeOff } from "lucide-react";
import { useState, type FormEvent } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { AccountPayload, AuthSnapshot } from "../bridge/types";

export function AuthGate({ auth, onAuthenticated }: { auth: AuthSnapshot; onAuthenticated(payload: AccountPayload): void }) {
  const [isSignUp, setIsSignUp] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [selfHosted, setSelfHosted] = useState(auth.selfHosted);
  const [backendUrl, setBackendUrl] = useState(auth.backendUrl ?? "");
  const [publishableKey, setPublishableKey] = useState("");
  const [backendState, setBackendState] = useState(auth);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const comparableUrl = (value: string) => value.trim().replace(/\/+$/, "");
  const savedKeyMatches = selfHosted
    && backendState.selfHosted
    && backendState.customKeySaved
    && comparableUrl(backendUrl) === comparableUrl(backendState.backendUrl ?? "");
  const backendReady = selfHosted
    ? !!backendUrl.trim() && (!!publishableKey.trim() || savedKeyMatches)
    : backendState.officialBackendConfigured;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setMessage(null);
    try {
      const configured = await invoke<{ auth: AuthSnapshot }>("auth.configureBackend", {
        selfHosted,
        backendUrl: selfHosted ? backendUrl : undefined,
        publishableKey: selfHosted ? publishableKey : undefined,
      });
      setBackendState(configured.auth);
      if (selfHosted) setPublishableKey("");
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
          <label className="self-host-toggle">
            <input
              type="checkbox"
              checked={selfHosted}
              disabled={busy}
              onChange={(event) => { setSelfHosted(event.target.checked); setMessage(null); }}
            />
            <span><strong>Self-hosted backend</strong><small>Connect this client to your own Nuvio server.</small></span>
          </label>
          {selfHosted && <fieldset className="self-host-fields">
            <label>Backend URL<input type="url" inputMode="url" autoCapitalize="none" autoCorrect="off" spellCheck={false} value={backendUrl} onChange={(event) => setBackendUrl(event.target.value)} placeholder="https://nuvio.example.com" required /></label>
            <label>Publishable key<input type="password" autoComplete="off" value={publishableKey} onChange={(event) => setPublishableKey(event.target.value)} placeholder={savedKeyMatches ? "Saved — leave blank to keep it" : "Your Supabase publishable key"} required={!savedKeyMatches} /></label>
            <small>The URL and public client key stay on this device. HTTPS is strongly recommended for remote servers.</small>
          </fieldset>}
          <label>Email<input type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} placeholder="you@example.com" required /></label>
          <label>Password<div className="password-field"><input type={showPassword ? "text" : "password"} autoComplete={isSignUp ? "new-password" : "current-password"} minLength={6} value={password} onChange={(event) => setPassword(event.target.value)} placeholder="At least 6 characters" required /><button type="button" aria-label={showPassword ? "Hide password" : "Show password"} title={showPassword ? "Hide password" : "Show password"} aria-pressed={showPassword} onClick={() => setShowPassword((value) => !value)}>{showPassword ? <EyeOff /> : <Eye />}</button></div></label>
          <button className="auth-submit" disabled={busy || !backendReady}>{busy ? "Connecting…" : isSignUp ? "Create account" : "Sign in"}</button>
        </form>

        {!selfHosted && !backendState.officialBackendConfigured && <div className="auth-message error">This build has no official backend configuration. Select self-hosted and enter your server details.</div>}
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
