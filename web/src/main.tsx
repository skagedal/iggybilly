import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { PlayerProvider } from "./player";
import { Router } from "./router";
import { loadPage, readInitialEnvelope } from "./pageData";

/**
 * The single frontend entry. Every page loads this; which page module it
 * then renders comes from the envelope the server embedded in the shell.
 *
 * Awaiting the first module before the initial render costs one extra
 * round trip on a cold load, but means the page appears fully formed
 * rather than flashing an empty shell first.
 */
const envelope = readInitialEnvelope();
const initialComponent = await loadPage(envelope.entry);

const container = document.getElementById("root");
if (!container) throw new Error("page shell is missing #root");

createRoot(container).render(
  <StrictMode>
    {/* PlayerProvider is outside Router on purpose: the bar it renders
        has to outlive every page swap, which is the whole point. */}
    <PlayerProvider>
      <Router initialEnvelope={envelope} initialComponent={initialComponent} />
    </PlayerProvider>
  </StrictMode>,
);
