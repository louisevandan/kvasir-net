/* ==========================================================================
   Tech-blog localization — merges per-language translations (tr-<lang>.ts)
   over the English source articles (articles.ts). Structure (slug, category,
   date, tags) always comes from TECH_ARTICLES; title/dek/blocks are replaced
   when the active language provides them, with English as the fallback.
   ========================================================================== */

import type { LangCode } from "../i18n/langs";
import { TECH_ARTICLES, type TechArticle, type TechTranslation } from "./articles";
import { koTech } from "./tr-ko";
import { zhTech } from "./tr-zh";
import { jaTech } from "./tr-ja";
import { esTech } from "./tr-es";
import { frTech } from "./tr-fr";
import { deTech } from "./tr-de";
import { nlTech } from "./tr-nl";
import { idTech } from "./tr-id";

const TRANSLATIONS: Partial<Record<LangCode, Record<string, TechTranslation>>> = {
  ko: koTech,
  zh: zhTech,
  ja: jaTech,
  es: esTech,
  fr: frTech,
  de: deTech,
  nl: nlTech,
  id: idTech,
};

export function getTechArticles(lang: LangCode): TechArticle[] {
  const tr = TRANSLATIONS[lang];
  if (!tr) return TECH_ARTICLES;
  return TECH_ARTICLES.map((a) => (tr[a.slug] ? { ...a, ...tr[a.slug] } : a));
}
