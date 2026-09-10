import { TooltipProvider } from "@/components/ui/tooltip";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "@/components/ui/sonner";
import App from "./App";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider>
    <App />
    <Toaster
      className="pointer-events-auto"
      theme="light"
      position="top-right"
      offset={16}
      richColors
      closeButton
      duration={4500}
      containerAriaLabel="通知"
      toastOptions={{ closeButtonAriaLabel: "关闭通知" }}
    />
    </TooltipProvider>
  </StrictMode>,
);
