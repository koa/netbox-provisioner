# Konzept: Protokoll im NetBox-Journal

Status: zum Teil umgesetzt ([Protokoll](../protocol.md)). Rest: Aufgabe C15 in
[todo.md](../todo.md). Entscheidungen vom 2026-10-25 eingearbeitet (Z29 bis Z33).

## Warum

Die [Jobs](../jobs.md) leben im Speicher des Prozesses, mit ihren Events. Nach einem Neustart des
Servers zeigt die Geräteseite einen früheren Job nur noch als „Auftrag unterbrochen“, ohne zu
sagen, wie weit er kam („Skript angewendet, Fallback scharf, dann nichts mehr“). Was auf dem Gerät
von selbst gewirkt hat (ein ausgelöster Fallback), erfährt niemand, wenn kein Job es gesehen hat.

## Was schon gebaut ist

Ein Eintrag pro Job und Gerät im Journal des Geräts, nachgeführt bei jedem Zustand, mit Tag
`netbox-provisioner` und Custom Field `provisioner_job`, die letzten 20 pro Gerät, geschrieben mit
`netbox_write_token` (Z29, Z30, Z32, Z33): [Protokoll](../protocol.md). Offen ist der Rest unten.

## Nach einem Neustart

Beim Start liest der Server seine Einträge, die noch als laufend markiert sind. Gehört einer zu
einem weitergeführten Job (`saved_job` hält dafür die Id des Eintrags), schreibt dieser Job darin
weiter. Alle anderen setzt er auf „unterbrochen in X“ (`warning`), mit dem, was der Eintrag als
auf dem Gerät liegend nennt.

Die Geräteseite zeigt unter den Jobs den **Verlauf**: die letzten Einträge aus dem Journal, mit
Link nach NetBox. Die Job-Ansicht eines unbekannten Jobs verweist auf den Eintrag statt nur
„unterbrochen“ zu sagen.

## Beobachtungen ohne Job

*Entschieden 2026-10-25 (Z31):* auch festhalten, was ohne Job geschieht. Die Grundlage ist das,
was die Job-Einträge über das Gerät sagen (`provisioner_job`: was auf ihm liegt, als ein Job
endete).

- **Rest ohne Erklärung:** Findet die Geräteseite einen Rest ([Liegengebliebenes](../jobs.md#was-auf-dem-gerät-liegen-bleibt)),
  den kein laufender Job hält und kein Eintrag als liegend nennt (eine andere Instanz, ein älterer
  Stand, von Hand angelegt), schreibt sie einmal einen Eintrag `warning` „Rest gefunden“.
- **Fallback ausgelöst:** Nennt der letzte Eintrag einen Apply- oder Update-Fallback als liegend,
  und die Geräteseite findet ihn nicht mehr, während die Uptime kürzer ist als die Zeit seit dem
  Eintrag, hat er ausgelöst: ein Eintrag `danger`, beim Apply mit den Marken des Skripts, die noch
  auf dem Gerät liegen. Ist die Uptime länger, hat ihn jemand von Hand entfernt: `info`.
- **Weggeräumt:** Ein Rest, den ein Eintrag als liegend nannte und der ohne Aufräumen-Job weg ist,
  schliesst den Eintrag nicht neu; das Aufräumen über `clearLeftover` ist selbst ein Job mit
  Eintrag.

Beobachtet wird beim Lesen der Geräteseite (`AccessibleDevice.leftovers`), nicht im Hintergrund:
Der Provisioner fragt keine Geräte ab, die niemand anschaut.
