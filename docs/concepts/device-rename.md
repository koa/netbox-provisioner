# Geräte umbenennen

Entwurf zu E23. Ein Gerät bekommt in NetBox einen neuen Namen, und der Provisioner zieht alles
nach, was den alten Namen trägt. Welche Namen sinnvoll sind, steht im
[Namens-Leitfaden](../naming-guide.md); er ist kein Teil der Software.

## Was heute am Namen hängt

Geprüft im Code (2026-11-25):

| Wo | Was der Name dort tut | Nach einer Umbenennung in NetBox |
|---|---|---|
| `/system/identity` | `addresses.rs` setzt den NetBox-Namen | der Diff setzt den neuen; bis dahin warnt der Kopf der Geräteseite („Das Gerät heisst „…“, nicht „…““) |
| Anmeldung | `other_device_named`: meldet sich das Gerät mit dem Namen eines **anderen** NetBox-Geräts, gilt es als falsches Gerät (`WrongDevice`) | blockiert, wenn der alte Name inzwischen einem anderen Gerät gehört (Tausch, Kette A→B, B→C) |
| Port-Namen der Nachbarn | ein Port ohne Label heisst nach dem Gerät am Kabelende (`e02-hegglin`, [Namensgenerierung](../interface-naming.md)) | die Nachbarn bekommen im Diff neue Port-Namen |
| Labels der Nachbarn | „Verbindung“ und die Funkbrücke setzen als Label den Namen der Gegenseite | bleiben auf dem alten Namen; der Befund `LabelNotFarEnd` meldet sie |
| WebFig-Zertifikat | der Name ist ein DNS-Name des Zertifikats (`www_certificate::wanted`) | das nächste Anwenden stellt ein neues aus |
| CAPsMAN | Namen der dynamischen Interfaces eines CAP (`cap-<identity>`) | folgen der neuen Identity (nicht geprüft) |
| Metriken | Label `device` | eine neue Zeitreihe; die alte endet |
| Gespeicherte Jobs, Rollout-Stand | nach NetBox-ID | nicht betroffen |
| ausserhalb | DNS (jq-Skript aus NetBox für Blocky), Prometheus-Ziele, Lesezeichen | folgen NetBox von selbst oder von Hand |

Der Provisioner verträgt also eine Umbenennung schon heute, solange der neue Name frei ist und
der alte keinem anderen Gerät gehört. Es fehlen zwei Dinge: Die Labels der Nachbarn bleiben alt,
und ein Tausch von Namen sperrt die Anmeldung.

## Vorschlag

### Helfer „Umbenennen“ auf der Geräteseite

Ein Helfer wie die anderen (Plan, Vorschau, Schreiben, Rückgängig), nur NetBox:

1. Eingabe: der neue Name. Abgelehnt wird ein Name, den schon ein anderes Gerät trägt; gewarnt
   wird, wo er keinem DNS-Namen entspricht (dann kein Name im Zertifikat, siehe Leitfaden).
2. Plan:
   - `UpdateDevice`: der Name (neuer Schritt, mit dem alten Namen für Rückgängig).
   - Für jedes Interface eines anderen Geräts, dessen Label den alten Namen nennt (dieselbe
     Regel wie `LabelNotFarEnd`, ohne Gross/Klein und Satzzeichen): `UpdateInterface` mit dem
     neuen Namen. Ein Label, das noch anderes enthält („Kunde“, „AP Keller“), bleibt.
3. Vorschau: die Diffs des Geräts und seiner Nachbarn (neue Port-Namen, neues Label), wie bei
   „Verbindung“; Geräte mit Änderungen ausserhalb der Labels gibt es nicht.

Ausgerollt wird wie jede NetBox-Änderung: von Hand mit „Gemeinsam anwenden“ oder, auf einer
automatischen Site, durch das Rollout. Das Gerät und seine Nachbarn ändern dabei Namen
(Identity, Port-Namen) und das Zertifikat. Ob ein Umbenennen einen Haltegrund des Rollouts
auslöst (etwa ein umbenannter Funk-Port), ist nicht geprüft.

### Tausch und Ketten

Mehrere Geräte bekommen in einem Plan neue Namen (der Helfer nimmt eine Liste: Gerät, neuer
Name). Der Plan prüft die Namen erst am Ende: Ein Tausch A↔B oder eine Kette A→B, B→C ist
erlaubt, solange danach jeder Name einmal vorkommt.

Danach meldet sich Gerät A mit dem Namen, den jetzt B trägt, und die Anmeldung schlägt mit
`WrongDevice` fehl. Dagegen:

- **Seriennummer:** Hat das NetBox-Gerät eine Seriennummer und meldet das Gerät dieselbe
  (`/system/routerboard serial-number`, in mikrotik-model modelliert; NetBox: `serial`, schon
  gelesen), gilt es als das richtige, auch wenn seine Identity einem anderen
  NetBox-Gerät gehört. Die Seriennummer ist ein Merkmal des Geräts wie der Name, keine MAC.
- **Ohne Seriennummer:** Die Geräteseite zeigt den Konflikt und lässt den Admin bestätigen,
  dass es das richtige Gerät ist (neu: ein anderes Board warnt heute nur, ohne Bestätigung). Die
  Bestätigung gilt für diese Identity, bis das Anwenden sie ändert.

Das Rollout fasst ein solches Gerät nicht an, bis es sich richtig meldet; es bleibt ein Fall für
die Geräteseite.

### Was nicht dazugehört

- Keine automatische Umbenennung nach dem Leitfaden: Der Leitfaden hilft beim Wählen, der
  Provisioner prüft ihn nicht.
- Keine Weiterleitung alter Namen (DNS, Metriken): Die alte Zeitreihe endet, wie bei einem
  Gerätetausch.
- Interfaces, VLANs, Sites und Locations: Ihre Namen hängen nicht am Gerätenamen.

## Offen

- Ob der Helfer auch Kabel-Labels ohne Gerätename korrigieren soll, die der Leitfaden anders
  will (nein im Vorschlag: nur was den alten Namen nennt).
- Ob die Seriennummer auch heute schon bei `WrongDevice` hilft (ein Gerät mit falsch gesetzter
  Identity, ohne Umbenennung).
