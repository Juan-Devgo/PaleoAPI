package co.juandevgo.paleoapi.application.ports.in;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;

import java.util.List;

public interface FindAllSpeciesTimePeriodUseCase {

    List<SpecieDTO> findAllSpeciesTimePeriod(String timePeriod);
}
