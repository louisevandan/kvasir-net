import type { Dict } from "./types";
import type { LangCode } from "./langs";
import { en } from "./en";
import { ko } from "./ko";
import { zh } from "./zh";
import { es } from "./es";
import { ja } from "./ja";
import { fr } from "./fr";
import { de } from "./de";
import { nl } from "./nl";
import { id } from "./id";

/* Registry of every language dictionary, keyed by code. */
export const DICTS: Record<LangCode, Dict> = {
  en,
  ko,
  zh,
  es,
  ja,
  fr,
  de,
  nl,
  id,
};
