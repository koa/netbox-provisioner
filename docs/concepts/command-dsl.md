# Konzept: RouterOS-Befehle ohne Strings

Status: Konzept, nicht umgesetzt. Offener Punkt „DSL für Mikrotik Kommandos“ aus
[pending.md](../pending.md#dsl-für-mikrotik-kommandos).

## Ausgangslage

Der grösste Teil ist schon getypt: Die Konfiguration entsteht aus den Modellen von
`mikrotik-model` (`ros_model/*.txt` → generierte Structs), der Diff aus `ResourceMutation`, und
das Skript rendert `generator.rs`. Als Strings bleiben zwei Arten:

1. **Einzelne API-Befehle** über `device::raw::command(device, path, &[(key, value)])`, rund 40
   Aufrufe: Aktionen ohne Modell (`/system/reboot`, `/partitions/activate`,
   `/system/package/uninstall`, `/interface/wifi/radio/provision`, `/ping`, `/execute`), das
   Entfernen der eigenen Scheduler und Skripte und einzelne Felder, die nur gelesen werden. Pfad und
   Attributnamen sind Strings; ein Tippfehler fällt erst am Gerät auf (`Error::Refused`).
2. **RouterOS-Skripte als `format!`**:
   - die `on-event`-Skripte der Scheduler (Fallback, Update-Fallback, Watchdog-Wiederherstellung);
   - `:delay` vor dem Apply-Skript;
   - die Konsolenbefehle der Ersteinrichtung über mac-telnet (`/ip address add …` mit
     `:while`-Schleifen, `/user set … password=…`).

   Hier steckt das Risiko: Quoting (`quote()` in `bootstrap.rs`), `$`-Variablen, geschweifte
   Klammern, die `format!` verdoppelt haben will.

## Ziel

Fehler, die man ohne Gerät finden kann, ohne Gerät finden: falscher Pfad, falscher Feldname,
falscher Werttyp, kaputtes Quoting. Lesbar bleiben; ein RouterOS-Kundiger soll das Skript im
Code noch wiedererkennen.

## Vorschlag

**Stufe 1: Befehle aus dem Modell.** Für Menüs, die `mikrotik-model` kennt, gibt es die Pfade und
Feldnamen schon (`RosResource::path()`, die Feld-Keys der Structs). Ein `command`-Aufruf auf
einem bekannten Menü bekommt eine getypte Variante:

```rust
raw::set::<SystemWatchdogCfg>(device, |w| w.watch_address(address)).await?;
raw::remove_named::<SystemSchedulerByName>(device, NAME).await?;
```

Der Generator erzeugt dafür pro Feld einen Setter, der den `RosValue` des Feldes kodiert. Aktionen
ohne Modell (`reboot`, `activate`, `provision`) bekommen je eine kleine Funktion in `device/raw.rs`
mit getypten Parametern; der String steht dann genau einmal im Code.

**Stufe 2: Skripte als Ausdrucksbaum.** Ein kleines Modul `ros_script` mit den paar Konstrukten,
die wirklich vorkommen: Befehl mit Argumenten (aus Stufe 1), `[find …]`, `:local`, `:if`,
`:while`, `:delay`, `:do`. `Display` rendert es mit korrektem Quoting; Werte gehen immer durch
`RosValue`. Ein Makro (`ros_script! { … }`) ist möglich, lohnt sich aber erst, wenn die Bäume von
Hand unhandlich werden. Bei den heute etwa zehn Skripten wahrscheinlich nicht.

**Prüfung:** Jedes erzeugte Skript bekommt einen Unit-Test gegen den erwarteten Text, wie die
Generator-Tests, und die Skripte der Scheduler laufen in den CHR-Tests wirklich
(`watchdog_is_restored_or_restores_itself` tut das schon für eines).

## Was nicht

Kein Ersatz für den Konfigurations-Generator, der ist schon getypt. Keine vollständige
RouterOS-Skriptsprache: nur, was der Provisioner selbst schreibt.

## Reihenfolge

1. Die Aktionen in `raw.rs` als Funktionen (Strings einmal, Parameter getypt).
2. Stufe 1 für die Menüs mit Modell (Scheduler, Script, Watchdog, File).
3. Stufe 2 für die mac-telnet-Befehle der Ersteinrichtung (dort ist das Quoting am heikelsten).

## Entschieden (2026-10-23)

- **Vorerst nur Stufe 1** (getypte Befehle aus dem Modell). Stufe 2 überzeugt noch nicht und
  ist nicht geplant.
