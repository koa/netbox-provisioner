# Konzept: Protokoll

Status: offen (C15). Zu klären: wie es aussieht, wo es liegt und wie lange es bleibt.

## Warum

Die [Jobs](../jobs.md) leben im Speicher des Prozesses, mit ihren Events. Nach einem Neustart des
Servers zeigt die Geräteseite einen früheren Job nur noch als „Auftrag unterbrochen“, ohne zu
sagen, wie weit er kam („Skript angewendet, Fallback scharf, dann nichts mehr“). Was auf dem Gerät
von selbst gewirkt hat (ein ausgelöster Fallback), erfährt niemand, wenn kein Job es gesehen hat.

## Was ins Protokoll gehört

Was geschehen ist, nicht was noch läuft (das sagt der Zustand des Jobs):

- das Ende jedes Auftrags (fertig, fehlgeschlagen, abgebrochen, unterbrochen), mit dem Zustand, in
  dem er endete, und seinen Events;
- ein ausgelöster Fallback oder ein Sicherheitsnetz, das die Watchdogs selbst wieder gesetzt hat;
  beim Apply mit den Marken des Skripts;
- ein teilweise geschriebener NetBox-Plan (wie bei `poe_type` an koenig-hegglin);
- ein Rest auf einem Gerät, den kein bekannter Auftrag hinterlassen hat (eine andere Instanz, ein
  älterer Stand; Arbeitsname `Fremd`).

Ein Übergang eines Jobs kann einen Eintrag schreiben; der Zustand verweist nie auf ihn.

## Was sich für die Bedienung ändern soll

- Nach einem Neustart des Servers steht bei jedem betroffenen Gerät „unterbrochen in X“ mit den
  Events bis dahin statt eines leeren Jobs.
- „Liegengebliebenes“ kann sagen, welcher Auftrag einen Rest angelegt hat.
- Die Site-Übersicht kann zeigen, welche Geräte etwas Offenes haben.

## Offen

- Form: Events als JSON-Zeilen in `state_dir` (wie die Dateien der weiterführbaren Aufträge),
  oder etwas anderes?
- Wie lange Einträge bleiben, und ob die Geräteseite sie direkt zeigt oder auf Nachfrage.
- Ob ein ausgelöster Fallback beobachtet wird (beim nächsten Lesen der Geräteseite, oder vom Job,
  der weiterläuft) oder nur, was ein Job sieht.
