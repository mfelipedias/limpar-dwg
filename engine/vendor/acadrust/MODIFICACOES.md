# Modificações nesta cópia da acadrust 0.6.8

Cópia da [acadrust](https://github.com/hakanaktt/acadrust) 0.6.8 (MPL-2.0), alterada para o nanoclean.
Os arquivos modificados continuam sob a MPL-2.0 (veja `LICENSE`).

1. `src/io/dwg/dwg_stream_writers/object_writer/entities.rs` (`write_vertex2d`):
   os vértices de polilinhas 2D eram gravados sempre com linetype `ByLayer`, escala 1 e espessura
   `ByLayer`. Agora herdam esses valores da polilinha dona, como o AutoCAD exige. Antes, o AUDIT do
   AutoCAD acusava "AcDb2dVertex linetype != owner's".

2. `src/io/dwg/dwg_document_builder.rs` (`preserve_raw_entity` e ramo de falha da segunda passada):
   no modo *failsafe*, uma entidade cujo conteúdo não pode ser decodificado (por exemplo, as SURFACE
   da planta do Jabaquara) era descartada. Agora ela é mantida como `EntityType::Unknown` com os bytes
   originais do registro, e o gravador a copia intacta para o DWG de saída. A falha continua sendo
   notificada, com a mensagem "Preserved entity ...".
