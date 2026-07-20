/* ==========================================================================
   Wiki localization — merges per-language translations (tr-<lang>.ts) over
   the English source entries (entries.ts). Structure (slug, category, image)
   always comes from WIKI_ENTRIES; title/summary/blocks are replaced when the
   active language provides them, with English as the fallback.
   ========================================================================== */

import type { LangCode } from "../i18n/langs";
import { WIKI_ENTRIES, type WikiEntry, type WikiTranslation } from "./entries";
import { koWiki } from "./tr-ko";
import { zhWiki } from "./tr-zh";
import { esWiki } from "./tr-es";
import { jaWiki } from "./tr-ja";
import { frWiki } from "./tr-fr";
import { deWiki } from "./tr-de";
import { nlWiki } from "./tr-nl";
import { idWiki } from "./tr-id";

const TRANSLATIONS: Partial<Record<LangCode, Record<string, WikiTranslation>>> = {
  ko: koWiki,
  zh: zhWiki,
  es: esWiki,
  ja: jaWiki,
  fr: frWiki,
  de: deWiki,
  nl: nlWiki,
  id: idWiki,
};

export function getWikiEntries(lang: LangCode): WikiEntry[] {
  const tr = TRANSLATIONS[lang];
  if (!tr) return WIKI_ENTRIES;
  return WIKI_ENTRIES.map((e) => (tr[e.slug] ? { ...e, ...tr[e.slug] } : e));
}
