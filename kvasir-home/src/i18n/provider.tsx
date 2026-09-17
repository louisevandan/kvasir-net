import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { Dict } from "./types";
import { DICTS } from "./dicts";
import { DEFAULT_LANG, isLangCode, type LangCode } from "./langs";

const STORAGE_KEY = "kvasir-lang";

type I18nValue = {
  lang: LangCode;
  setLang: (l: LangCode) => void;
  t: Dict;
};

const I18nContext = createContext<I18nValue | null>(null);

/** The language named by the path, if the URL carries one: /ko, /ja/wiki, … */
export function langFromPath(pathname: string): LangCode | null {
  const first = pathname.split("/").filter(Boolean)[0];
  return isLangCode(first) ? first : null;
}

/** The same route without its language prefix, for switching languages. */
export function stripLang(pathname: string): string {
  const lang = langFromPath(pathname);
  if (!lang) return pathname || "/";
  const rest = pathname.slice(lang.length + 1);
  return rest || "/";
}

/**
 * The language of this page is the language in the URL — and at a bare URL it
 * is English, full stop.
 *
 * Guessing from the browser used to override it, so the canonical English page
 * rendered in Korean for a Korean visitor: the indexed document and the served
 * document disagreed, which is the one thing a crawler cannot forgive. A stored
 * preference still applies, but by *moving* the reader to that language's URL
 * (see the redirect below), never by quietly changing what this URL says.
 */
function resolveInitial(): LangCode {
  if (typeof window === "undefined") return DEFAULT_LANG;
  return langFromPath(window.location.pathname) ?? DEFAULT_LANG;
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<LangCode>(resolveInitial);

  useEffect(() => {
    document.documentElement.lang = lang;
    try {
      window.localStorage.setItem(STORAGE_KEY, lang);
    } catch {
      /* storage unavailable — ignore */
    }
    // Keep the address bar honest about which language is on screen, so the URL
    // stays shareable and indexable. Replace rather than push: switching
    // language is not a new page in the reader's history.
    if (typeof window === "undefined") return;
    const bare = stripLang(window.location.pathname);
    const next = lang === DEFAULT_LANG ? bare : `/${lang}${bare === "/" ? "" : bare}`;
    if (next !== window.location.pathname) {
      window.history.replaceState(null, "", next + window.location.search + window.location.hash);
    }
  }, [lang]);

  // A reader who chose a language once arrives later at a bare URL. Send them
  // to that language's own URL rather than rendering it here, so what the page
  // says and what its address claims never come apart.
  useEffect(() => {
    if (typeof window === "undefined") return;
    if (langFromPath(window.location.pathname)) return;
    let stored: string | null = null;
    try { stored = window.localStorage.getItem(STORAGE_KEY); } catch { /* ignore */ }
    if (!isLangCode(stored) || stored === DEFAULT_LANG) return;
    const bare = stripLang(window.location.pathname);
    window.location.replace(`/${stored}${bare === "/" ? "" : bare}${window.location.search}${window.location.hash}`);
  }, []);

  const value = useMemo<I18nValue>(
    () => ({ lang, setLang: setLangState, t: DICTS[lang] ?? DICTS[DEFAULT_LANG] }),
    [lang]
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

function useI18n(): I18nValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used within <I18nProvider>");
  return ctx;
}

/** The active dictionary. */
export const useT = (): Dict => useI18n().t;

/** The active language code + setter. */
export function useLang() {
  const { lang, setLang } = useI18n();
  return { lang, setLang };
}
