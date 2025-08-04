package co.juandevgo.paleoapi.application.ports.in;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;

import java.util.List;

public interface FindAllSpeciesNameUseCase {

    List<SpecieDTO> findAllSpeciesName(String name);
}