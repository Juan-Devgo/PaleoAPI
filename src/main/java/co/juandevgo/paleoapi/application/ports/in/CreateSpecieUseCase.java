package co.juandevgo.paleoapi.application.ports.in;

import co.juandevgo.paleoapi.application.dto.CreateSpecieResponseDTO;
import co.juandevgo.paleoapi.application.dto.SpecieDTO;

public interface CreateSpecieUseCase {

    CreateSpecieResponseDTO createSpecie(SpecieDTO specie);
}