import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./index.css";
import { api } from "./lib/api";
import { setSiteAppearance } from "./lib/theme";

// The cached default from the last visit is already applied; this only catches admin changes.
api.publicInfo().then((p) => setSiteAppearance(p.appearance), () => {});

const root = document.getElementById("root");
if (root) createRoot(root).render(<StrictMode><App /></StrictMode>);
