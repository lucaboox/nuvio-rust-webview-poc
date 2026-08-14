import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { AppErrorBoundary } from "./components/AppErrorBoundary";
import "./styles/app.css";

// Preserve Nuvio's custom right-click handlers while suppressing the native
// browser menu exposed by Tauri's WebView.
document.addEventListener("contextmenu", (event) => event.preventDefault());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppErrorBoundary>
      <App />
    </AppErrorBoundary>
  </StrictMode>,
);
