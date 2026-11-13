import { api } from "../api";
import { clearDrafts } from "../models/compose";
import { clearOutbox } from "./outbox";

export async function signOut() {
  try {
    await api.signOut();
  } finally {
    // Native sign-out can purge credentials even when settings/cleanup fails.
    clearDrafts();
    clearOutbox();
  }
}
