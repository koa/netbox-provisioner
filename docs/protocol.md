# Protokoll im NetBox-Journal

Die [Jobs](jobs.md) leben im Speicher des Provisioners. Damit nach einem Neustart noch zu sehen
ist, was ein Job an einem Gerät getan hat und wie weit er kam, schreibt jeder Job eines Geräts
einen Eintrag ins Journal des Geräts in NetBox (`extras/journal-entries`, Z29). Code:
`backend/src/journal.rs`, Texte in `i18n/src/journal.rs`.

## Ein Eintrag pro Job

- **Angelegt beim Start** des Jobs (`kind` `info`, „läuft“), **nachgeführt** bei jedem Zustand,
  den er betritt (`Log::enter`), und an seinem Ende (`PATCH`). So steht auch nach einem Absturz
  mitten im Job da, in welchem Zustand er zuletzt war. Ein eigener Task pro Job schreibt; kommen
  Zustände schneller als NetBox antwortet, schreibt er nur den neuesten.
- **`kind` am Ende:** fertig `success`; fertig, aber nicht alles angewendet (ein Rest im Diff,
  ein ausgelöster Fallback, ein Batch, der nicht durchkam, etwas bleibt auf dem Gerät) oder
  abgebrochen `warning`; fehlgeschlagen `danger`.
- **Text** (Markdown, ein Absatz pro Zeile): Art und Ausgang („**Anwenden** abgeschlossen“), wer
  den Job gestartet hat (Name oder E-Mail der Anmeldung, „nach einem Neustart weitergeführt“ bei
  einem weitergeführten), der Zustand, in dem er ist oder endete (nicht bei fertig), der Fehler,
  die wichtigen Ergebnisse (mit welchen Geräten gemeinsam angewendet, nichts anzuwenden,
  angewendet oder nicht alles, Fallback ausgelöst, RouterOS von → nach, Strom aus und ein, Anzahl
  geschriebener NetBox-Objekte), was auf dem Gerät bleibt (Fallback des Anwendens oder des Updates,
  Sicherheitsnetz der Watchdogs) und die Dauer. Nicht jedes Event: die Einzelheiten zeigt die
  Job-Ansicht, solange der Provisioner den Job kennt.
- **Gemeinsam anwenden:** ein Eintrag pro Gerät, der die anderen nennt.
- **Nicht ins Journal:** die Abweichung einer Site (`SiteDiff`, liest nur, ist kein Job eines
  Geräts). Die Kaltstarts per PoE (`PowerCycle`) stehen beim speisenden Gerät, wie der Job.

## Erkennen und Auswerten

Die Einträge tragen den Tag `netbox-provisioner`. Was der Provisioner später auswertet, steht im
Custom Field `provisioner_job` (JSON) des Eintrags, nicht im Text: Art des Jobs, Ausgang,
Zustand, was auf dem Gerät bleibt, Zeitpunkt ([custom-fields.md](custom-fields.md#provisioner_job-journal-entry-json)).
Tag und Feld müssen in NetBox angelegt sein ([netbox-import/](netbox-import/)); ohne sie lehnt
NetBox jeden Eintrag ab.

## Freigabe und Fehler

Geschrieben wird, wann immer `netbox_write_token` gesetzt ist, unabhängig von
`allow_netbox_write`, das die Helfer freigibt (Z33). Ein Fehler beim Schreiben bricht nie einen
Job ab: Der Job zeigt einmal die Warnung `JournalNotWritten` (mit dem Fehler, „Eintrag im
NetBox-Journal nicht geschrieben“), und der nächste Zustand versucht es wieder; erst ein
Schreiben, das wieder gelingt, lässt einen neuen Fehler wieder melden.

## Dauer

Pro Gerät bleiben die neuesten 20 Einträge des Provisioners (Z30); ältere löscht der Job nach
seinem Ende. Einträge ohne den Tag (von Menschen) bleiben unberührt.

## Sprache

Der Text ist Deutsch aus dem Crate `i18n`, das auch das Frontend nutzt (Z32): dieselben Namen
der Job-Arten und Zustände wie in der Job-Ansicht, Fehler über `i18n::error::server`
(`ErrorInfo` setzt `i18n::error::Problem` um). Das Backend baut keinen Satz selbst.

## Nach einem Neustart

Ein weitergeführter Job schreibt in seinem Eintrag von vorher weiter (`SavedJob::journal_entry`,
`Started::Resumed`); ist der in NetBox gelöscht, legt er einen neuen an. Alle anderen Einträge des
Provisioners, die noch laufen (`kind` `info`, `outcome` `Running`), setzt der Start auf
„unterbrochen: der Provisioner wurde neu gestartet“ (`warning`, `journal::close_interrupted`, am
Ende von `resume_saved_jobs`); der Zustand („Stand: …“) und was auf dem Gerät bleibt, stehen
weiter darin.

## Verlauf auf der Geräteseite

Die Geräteseite zeigt zuunterst den **Verlauf** (`components::history`,
`DeviceAccess.journal`, gelesen mit `netbox_token`): die Einträge des Provisioners, neueste
zuerst, mit Art und Ausgang, Zeitpunkt (wie NetBox ihn gibt, in seiner Zeitzone), wer gestartet
hat, dem Zustand (nicht bei fertig), was auf dem Gerät bleibt, und dem Link auf den Eintrag in
NetBox. Der Rand zeigt den Ausgang (grün fertig, gelb nicht alles, abgebrochen oder
unterbrochen, rot fehlgeschlagen, blau läuft). Gelesen wird aus `provisioner_job`, nicht aus dem
Text; ein Eintrag mit dem Tag ohne lesbares Feld fehlt. Nach jedem Job der Seite wird er neu
gelesen. „Auftrag unterbrochen“ in der Job-Ansicht verweist auf ihn.

## Beobachtungen ohne Job

Was auf dem Gerät ohne Job geschieht, hält die Geräteseite fest (Z31), wenn sie die Reste liest
(`AccessibleDevice.leftovers`, während kein Job des Provisioners auf dem Gerät läuft;
`journal::observe`, im Hintergrund). Der Provisioner fragt keine Geräte ab, die niemand
anschaut.

- **Was liegen sollte:** aus den Einträgen des Provisioners, ältester zuerst. Jeder Job-Eintrag
  nennt, was er liegen lässt (`left`) und welche Arten er angefasst hat (`touched`: einen Fallback
  gesetzt, Watchdogs ausgesetzt, einen Rest weggeräumt); was ein Job anfasst, zählt danach nur
  noch, soweit er es liegen lässt. Läuft der neueste Eintrag noch, wird nichts verglichen.
- **Rest gefunden** (`warning`): auf dem Gerät, ohne dass ein Eintrag ihn nennt (eine andere
  Instanz, ein älterer Stand, von Hand).
- **Ausgelöst** (`danger`): ein Apply- oder Update-Fallback, den ein Eintrag als liegend nennt,
  ist weg, und die Uptime ist kürzer als die Zeit seit diesem Eintrag.
- **Nicht mehr da** (`info`): weg ohne Neustart seither (von Hand entfernt; das Sicherheitsnetz
  der Watchdogs hat sie wiederhergestellt).

Ein solcher Eintrag (`provisioner_job.observation` mit `found`, `fired`, `removed`) nennt, was
jetzt auf dem Gerät liegt, und gilt als Stand: Das nächste Lesen findet nichts Neues und schreibt
keinen zweiten. Zwei Lesungen zugleich schreiben einen. Der Verlauf zeigt ihn als „Beobachtung auf
dem Gerät, ohne Job“. Geprüft im Lab mit einem Eintrag, der einen Fallback von vor dem Neustart
von lab-wap als liegend nannte: ein Eintrag „ausgelöst“, beim zweiten Lesen keiner mehr.

Noch nicht dabei sind die Marken des Skripts eines ausgelösten Apply-Fallbacks (E16).
