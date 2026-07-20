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

function resolveInitial(): LangCode {
  if (typeof window === "undefined") return DEFAULT_LANG;
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (isLangCode(stored)) return stored;
  const nav = window.navigator.language?.slice(0, 2).toLowerCase();
  if (isLangCode(nav)) return nav;
  return DEFAULT_LANG;
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
  }, [lang]);

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
