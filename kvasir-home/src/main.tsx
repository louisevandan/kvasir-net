import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import App from "./App.tsx";
import RunNodePage from "./components/RunNodePage.tsx";
import CareersPage from "./components/CareersPage.tsx";
import TechnologyPage from "./components/TechnologyPage.tsx";
import WikiPage from "./components/WikiPage.tsx";
import ApiDocsPage from "./components/ApiDocsPage.tsx";
import { I18nProvider } from "./i18n/provider";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <I18nProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<App />} />
          <Route path="/run-node" element={<RunNodePage />} />
          <Route path="/careers" element={<CareersPage />} />
          <Route path="/technology" element={<TechnologyPage />} />
          <Route path="/technology/:slug" element={<TechnologyPage />} />
          <Route path="/wiki" element={<WikiPage />} />
          <Route path="/wiki/:slug" element={<WikiPage />} />
          <Route path="/docs/api" element={<ApiDocsPage />} />
        </Routes>
      </BrowserRouter>
    </I18nProvider>
  </StrictMode>
);
