# OSPF-Authentisierung pro Segment

Aufgabe A10 (Frage Z44). Vor allem für die Ports von Anwohnern
([Self-Service](concepts/self-service.md#risiken)): Ein Gerät ohne den Schlüssel wird kein
OSPF-Nachbar und kann keine Routen einspeisen. Code: `device::ospf_auth` (die Entscheidung),
`device::ros::ospf` (die Templates), `device::batch` (Lesen vor dem Planen, Prüfung danach).

## Grundsatz

**Credentials werden nur angefasst, wenn positiv feststeht, dass alle Endpunkte des
L2-Segments im selben Anwenden sind. Im Zweifel: nicht anfassen.** Ohne diese Freigabe
übernimmt der Generator `auth`, `auth-id` und `auth-key` jedes Ports so, wie das Gerät sie hat
(`OspfCredentials`, `current_credentials`); es gibt keinen Weg, auf dem er von sich aus einen
Schlüssel setzt, ändert oder entfernt. Ohne die Einstellung `ospf_auth` wird nichts gelesen
und nichts entschieden.

## Was RouterOS tut (CHR 7.24.6, gemessen)

- `auth` gilt pro Interface-Template. Liegen zwei Ports im selben Template, bekommen beide
  denselben Schlüssel: Ein Segment für sich braucht ein Template für sich.
- `print` gibt `auth`, `auth-id` und `auth-key` zurück; ein Schlüssel aus 64 Hex-Zeichen geht
  mit `sha256`.
- Unterscheiden sich die Enden im Schlüssel, in `auth-id` oder in `auth`, fällt die
  Nachbarschaft nach dem Dead-Interval (38–40 s) weg, auch eine bestehende; v2 und v3 gleich.
- Wechseln beide Enden zugleich oder 20 s versetzt, bleibt sie `Full`, ohne verlorenen Ping.
- Ein Port, der aus dem gemeinsamen Template in ein eigenes wandert, bleibt `Full`, wenn das
  eigene zuerst angelegt und der Port dann aus dem gemeinsamen genommen wird. Umgekehrt (erst
  heraus, dann das eigene), wie der Diff zuerst sortierte, war sie im CHR-Test rund 45 s weg
  und der Ping über das Segment rund 50 s; daher `templates_added_first` nach dem Sortieren.
- Wechselt nur ein Ende, fällt die Nachbarschaft nach 40 s; sind beide wieder gleich, ist sie
  nach 9 s `Full`, Verkehr geht nach weiteren 7 s wieder (die Route kommt nach `Full`).

## Schlüssel

- Einstellung `ospf_auth`: `current`, die Zielversion (1–255, `auth-id` auf den Geräten), und
  `secrets`, ein Geheimnis pro Version (`APP_SETTINGS__OSPF_AUTH__SECRETS__<v>`; im Helm-Chart
  `settings.ospf_auth`, jedes Geheimnis ein `value` oder `secretKeyRef`). `current` muss unter
  den Geheimnissen sein, keins darf leer sein.
- Schlüssel eines Segments: `hex(HMAC-SHA256(secret[v], "<IPv4-Netz des Segments>"))`, 64
  Zeichen, `auth=sha256`, `auth-id=v` (`ospf_auth::target_credentials`). Beide Enden rechnen
  ihn aus denselben Daten; er steht nirgends ausser auf den Geräten.
- Wechsel des Geheimnisses: neue Version eintragen und `current` auf sie setzen. Jedes Segment
  wechselt, sobald ein Anwenden es freigibt; bis dahin behält es die alte Version (die
  `auth-id` auf dem Gerät sagt welche). Ein Geheimnis kann gehen, wenn kein Segment es mehr
  nutzt.

## Segment und Templates

Ein Segment ist ein IPv4-Netz mit `ospf`-Interfaces, nicht ein Kabel: ein Funkbrücken-Segment
hat vier Enden (zwei Router, zwei Brückengeräte), ein Switch ohne RouterOS dazwischen ändert
nichts. Ein Port mit genau einem IPv4-Netz hat ein Segment (`ospf_segments`), einer mit mehr
oder keinem bleibt unberührt.

`place_ospf_credentials` legt nach dem Generieren jeden Port dorthin, wo seine Credentials
hingehören: Das gemeinsame Template pro Area (`backbone-v2`/`-v3`, ohne Kommentar) behält, was
das gemeinsame Template des Geräts hat; ein Port mit anderen Credentials bekommt ein eigenes
(v2 und v3, sonst wie das gemeinsame, Kommentar `netbox-provisioner: ospf <Netz>`). Ein Port
kehrt ins gemeinsame zurück, sobald seine Credentials wieder dessen sind, das eigene geht dann.
Hat jeder Port ein eigenes, geht das gemeinsame (mit `interfaces=""` bliebe ein Template ohne
Port); der Diff macht dann aus ihm das eigene eines Ports.
Das eigene Template wird über sein Netz gefunden, so folgt es einem umbenannten Port. Sonst
behält ein Port die Credentials des Templates, in dem er auf dem Gerät steckt
(`current_credentials`, die Ports dort mit den Namen nach den Umbenennungen,
`ospf_ports_renamed`). So behält ein in NetBox umnummerierter Link (ein Anwohner hängt sein Gerät
ab und wieder an, „Kabel übernehmen“) seine Credentials. Vorher fiel ein Port, dessen Netz und
Name sich zugleich änderten, auf keine Authentisierung zurück (lab-wap, 2026-11-20).

## Freigabe

Vor dem Planen liest jedes Mitglied eines Anwendens seine OSPF-Ports (`DeviceCfg::ospf_ports`)
und -Nachbarn (`ospf_auth::read_neighbors`); dann entscheidet `ospf_auth::decide` über alle
zusammen. Ein Segment, das auf allen Mitgliedern schon die Zielversion hat, braucht nichts.
Jedes andere wird freigegeben, wenn **jede** Bedingung gilt:

1. **NetBox ist vollständig:** Jede Adresse im Netz gehört zu einem `ospf`-Interface eines
   RouterOS-Geräts mit primärer IPv4 (der Router-ID), mit derselben Präfixlänge, eine pro Gerät.
2. **Alle Enden sind im Anwenden:** jedes dieser Geräte ist Mitglied und gelesen.
3. **Live stimmt mit NetBox:** Auf jedem Mitglied sind die Nachbarn am Port, in v2 und v3,
   genau die anderen Enden (nach Router-ID), jeder `Full` oder `TwoWay`. Ein fremder oder ein
   fehlender Nachbar ist Zweifel: ein Segment, das unten ist, wird nicht angefasst. Ein
   Nachbar ohne Interface zählt an jedem Port.
4. **Alle Enden haben heute dasselbe:** gleiche `auth`, `auth-id` und `auth-key` in v2 und v3.

Freigegeben wird das Segment auf allen Enden auf die Zielversion gesetzt
(`DeviceCfg::take_ospf_decision`). Ein Segment, dessen einziges Ende das Gerät ist (der Port
eines Anwohners, dessen Gerät noch fehlt), wird so auch beim Anwenden allein freigegeben
(`decide_alone`); jedes andere nur in einem Batch (`applyBatch`, `decide_together`).

Die Vorschau zeigt dasselbe: `generateCfg` entscheidet wie ein Anwenden allein,
`generateCfg(together:)` wie ein Batch mit diesen Geräten (deren Ports und Nachbarn werden
dazu gelesen; eines, das nicht antwortet, fehlt, und seine Segmente bleiben). „Gemeinsam
anwenden“ fragt so für jedes Gerät der Auswahl, das eigene eingeschlossen, und fragt alle neu,
wenn die Auswahl sich ändert. Die Prüfung nach dem Skript rechnet mit der Entscheidung des
Plans (`AccessibleDevice.ospf_decision`), sonst sähe sie den Wechsel als Abweichung.

## Nach dem Skript

Für jeden gewechselten Port müssen die Nachbarn, die die Entscheidung erwartet
(`ExpectedNeighbors`), wieder `Full` oder `TwoWay` sein, bevor der Batch die Fallbacks
wegnimmt (`Watched::ospf_back`, jede Lesung mit 25 s Limit). Kommen sie nicht, bis der Batch
aufgibt, ist er unvollständig (`BatchIncomplete.ospf_missing`,
`FallbackLeftReason::OspfMissing`), und alle Fallbacks laden gemeinsam die Konfiguration von
vorher, mit den alten Schlüsseln.

CHR-Test `ospf_credentials_move_with_both_ends_of_a_segment` (`a -- b -- c`): a und b
zusammen, a–b wechselt, b–c bleibt mit `EndsOutside [c]`; b allein danach ohne Diff in den
Templates; b und c zusammen, b–c wechselt; dann Version 2, a allein lässt a–b, a und b
zusammen wechseln es. Die vier Nachbarschaften von b bleiben dabei `Full`, und ein Ping von a
an die Loopback von c geht jede Sekunde durch.

## Was der Operator sieht

Ein Segment, das nicht die Zielversion hat und nicht wechselt, ist der Hinweis
`OspfAuthKept` beim Anwenden (`ApplyWarning`), mit dem Grund (`OspfAuthDoubt`): nicht alle
Enden im Anwenden (welche fehlen), eine Adresse an einem Gerät ohne RouterOS oder ohne `ospf`,
ein fremder Nachbar (Router-ID), ein erwarteter Nachbar fehlt, die Enden haben verschiedene
Credentials.

## Bewusst nicht

Entschieden am 2026-11-18, zu viel Aufwand für den Nutzen:

- Die Automatik (C20) nimmt die anderen Enden eines ausstehenden Segments nicht in ihre
  Runde. Ein Segment wechselt, wenn der Operator alle Enden zusammen anwendet; bis dahin
  bleibt es, mit dem Hinweis.
- „Gemeinsam anwenden“ wählt die fehlenden Enden eines Segments nicht vor (nur die Partner
  der Funkstrecken); der Hinweis nennt sie.
- Ein Batch, der nach einem Neustart weiterläuft, hat keine Entscheidung: die Prüfung behält
  die Credentials, wie sie sind (kein Diff), wartet aber nicht auf die Nachbarn der
  gewechselten Segmente. In Kauf genommen: ein Ende, dessen Skript nicht durchkam, ist nicht in
  Reihe und schickt alle zurück.
