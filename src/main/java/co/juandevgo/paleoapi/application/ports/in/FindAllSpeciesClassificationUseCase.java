package co.juandevgo.paleoapi.application.ports.in;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;

import java.util.List;

public interface FindAllSpeciesClassificationUseCase {

    List<SpecieDTO> findAllSpeciesClassification(String classification);
}