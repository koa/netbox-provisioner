# Konzept: Protokoll im NetBox-Journal

Status: Konzept, nicht umgesetzt. Aufgabe C15 in [todo.md](../todo.md), vorher B11 (gemeinsames
i18n-Crate). Entscheidungen vom 2026-10-25 eingearbeitet (Z29 bis Z33).

## Warum

Die [Jobs](../jobs.md) leben im Speicher des Prozesses, mit ihren Events. Nach einem Neustart des
Servers zeigt die Geräteseite einen früheren Job nur noch als „Auftrag unterbrochen“, ohne zu
sagen, wie weit er kam („Skript angewendet, Fallback scharf, dann nichts mehr“). Was auf dem Gerät
von selbst gewirkt hat (ein ausgelöster Fallback), erfährt niemand, wenn kein Job es gesehen hat.

## Was ins Protokoll gehört

Was geschehen ist, nicht was noch läuft (das sagt der Zustand des Jobs):

- jeder Auftrag an ein Gerät mit seinem Ende (fertig, fehlgeschlagen, abgebrochen, unterbrochen),
  dem Zustand, in dem er endete, und dem, was er auf dem Gerät gelassen hat;
- **Beobachtungen ohne Job** (Z31): ein Rest auf dem Gerät, den kein Eintrag erklärt, und ein
  Fallback, der ausgelöst hat, ohne dass ein Job es sah.

Die Abweichung einer Site (`SiteDiff`) liest nur und kommt nicht ins Protokoll.

## Ablage: das Journal des Geräts in NetBox

*Entschieden 2026-10-25 (Z29):* Das Protokoll steht in NetBox, als Journal-Einträge
(`extras/journal-entries`) am Gerät. So sieht es jeder, der in NetBox nachschaut, es braucht kein
eigenes Volume, und es überdauert den Provisioner. Ein Journal-Eintrag hat `kind` (`info`,
`success`, `warning`, `danger`), `comments` (Markdown), Tags und Custom Fields, und er lässt sich
ändern (NetBox 4.7.1 im Lab).

- **Ein Eintrag pro Job und Gerät,** angelegt beim Start des Jobs (`info`, „läuft“) und bei jedem
  Zustandswechsel nachgeführt (`PATCH`). So steht auch nach einem Absturz mitten im Job da, wie
  weit er kam. Am Ende bekommt er seinen Ausgang als `kind`: fertig `success`, nicht alles
  angewendet oder abgebrochen `warning`, fehlgeschlagen `danger`.
- **Inhalt:** Art des Jobs, wer ihn gestartet hat, Start und Dauer, der letzte Zustand, die
  wichtigen Ergebnisse (Versionen eines Updates, Fallback gesetzt, gestoppt oder ausgelöst mit den
  Marken des Skripts, geschriebene NetBox-Objekte, der Fehler mit Code), und was auf dem Gerät
  bleibt. Nicht jedes Event: Die Konsolenausgabe und die Zwischenstände gehören in die Job-Ansicht,
  solange der Server den Job kennt.
- **Gemeinsam anwenden:** ein Eintrag pro Gerät, der die anderen nennt.
- **Erkennen der eigenen Einträge:** Tag `netbox-provisioner`. Was der Provisioner später
  auswerten muss (Job-Art, Zustand, was auf dem Gerät liegt, ob er noch läuft), steht im Custom
  Field `provisioner_job` (JSON) des Eintrags, nicht im Text; das Feld kommt in
  [custom-fields.md](../custom-fields.md).
- **Freigabe** (Z33): geschrieben wird, wann immer `netbox_write_token` gesetzt ist, unabhängig
  von `allow_netbox_write` (das bleibt die Freigabe der Helfer). Ein Fehler beim Schreiben bricht
  nie einen Job ab; er erscheint als Warnung im Job (`JournalNotWritten`), und der nächste
  Zustandswechsel versucht es wieder.
- **Dauer** (Z30): pro Gerät die letzten 20 Einträge des Provisioners; ältere löscht er nach dem
  Ende eines Jobs. Einträge von Menschen bleiben unberührt.

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

## Sprache

*Entschieden 2026-10-25 (Z32):* Die deutschen Texte kommen aus einem gemeinsamen Crate `i18n`,
das Frontend und Backend nutzen (B11). Die heutigen Module unter `frontend/src/i18n/` nehmen fast
nur einfache Werte (`&str`, Zahlen) und ziehen dorthin um; was `yew::Html` baut oder Typen des
Frontends kennt (`adjust_target`, `error`), bleibt im Frontend oder wird geteilt. Das Backend
formuliert damit weiterhin nichts selbst: Es nimmt die Texte aus dem Crate, wie das Frontend.
