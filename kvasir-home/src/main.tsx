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
import LegalPage from "./components/LegalPage.tsx";
import ReleasesPage from "./components/ReleasesPage.tsx";
import { I18nProvider } from "./i18n/provider";
import { LANGS, DEFAULT_LANG } from "./i18n/langs";
import { Routes as InnerRoutes, Route as InnerRoute } from "react-router-dom";

/**
 * A language-prefixed URL renders the same pages.
 *
 * The child paths are relative on purpose: this sits under a `/:lang/*` route,
 * so react-router already scopes them to that prefix. Absolute paths here match
 * nothing and the page renders blank — which looks exactly like a build
 * problem rather than a routing one.
 */
function Localized() {
  return (
    <InnerRoutes>
      <InnerRoute index element={<App />} />
      <InnerRoute path="run-node" element={<RunNodePage />} />
      <InnerRoute path="careers" element={<CareersPage />} />
      <InnerRoute path="technology" element={<TechnologyPage />} />
      <InnerRoute path="technology/:slug" element={<TechnologyPage />} />
      <InnerRoute path="wiki" element={<WikiPage />} />
      <InnerRoute path="wiki/:slug" element={<WikiPage />} />
      <InnerRoute path="docs/api" element={<ApiDocsPage />} />
      <InnerRoute path="legal" element={<LegalPage />} />
      <InnerRoute path="releases" element={<ReleasesPage />} />
    </InnerRoutes>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <I18nProvider>
      <BrowserRouter>
        <Routes>
          {/* Every route is served twice: bare for English, and under a
              language prefix. The prefixed form is what a crawler indexes and
              what hreflang points at; the app reads the language back off the
              path. */}
          {LANGS.filter((l) => l.code !== DEFAULT_LANG).map((l) => (
            <Route key={l.code} path={`/${l.code}/*`} element={<Localized />} />
          ))}
          <Route path="/" element={<App />} />
          <Route path="/run-node" element={<RunNodePage />} />
          <Route path="/careers" element={<CareersPage />} />
          <Route path="/technology" element={<TechnologyPage />} />
          <Route path="/technology/:slug" element={<TechnologyPage />} />
          <Route path="/wiki" element={<WikiPage />} />
          <Route path="/wiki/:slug" element={<WikiPage />} />
          <Route path="/docs/api" element={<ApiDocsPage />} />
          <Route path="/legal" element={<LegalPage />} />
          <Route path="/releases" element={<ReleasesPage />} />
        </Routes>
      </BrowserRouter>
    </I18nProvider>
  </StrictMode>
);
