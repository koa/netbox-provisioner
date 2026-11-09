Offene Punkte
============

Ein Punkt verschwindet von hier, sobald er erledigt ist oder als Aufgabe in [todo.md](todo.md)
steht (erledigte Aufgaben in [done.md](done.md)).

# Code Smells
## Tuples
Sei zurückhaltend mit der Verwendung von Tuples bei fn-calls. Meist ist es besser einen struct zu erstellen. Dabei erhalten die Komponenten Namen, welche die Absicht besser beschreiben.

## Strings und Bytes
Konstante Strings werden besser als Box<str> gespeichert. String macht nur dort sinn, wo eine API zwingend einen String erfordert oder der String mutiert werden muss (ähnlich: Box<[_]> statt Vec<_>).
Wenn ein String nur von RouterOs geladen und wieder gespeichert werden soll, passt Box<[u8]> noch besser (Die Strings müssen meist nicht nach utf8 und zurück konvertiert werden)

## dyn
Dyn führt dazu, dass der Compiler den Typ nicht mehr prüfen kann. Es kann in speziellen Ausnahmefällen sinnvoll sein. Meistens ist aber ein statischer Typ besser.