import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AnimateUIProvider } from "@animate-ui/react/provider";
import { App } from "@/App";
import { Toaster } from "@/components/ui/toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import "@/index.css";
import { api } from "@/lib/api";
import { setSiteAppearance } from "@/lib/theme";

// The cached default from the last visit is already applied; this only catches admin changes.
api.publicInfo().then((p) => setSiteAppearance(p.appearance), () => {});

const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <StrictMode>
      <AnimateUIProvider>
        <TooltipProvider>
          <App />
          <Toaster />
        </TooltipProvider>
      </AnimateUIProvider>
    </StrictMode>,
  );
