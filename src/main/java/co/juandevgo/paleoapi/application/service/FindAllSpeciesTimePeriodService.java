package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesTimePeriodUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class FindAllSpeciesTimePeriodService implements FindAllSpeciesTimePeriodUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesTimePeriod(String timePeriod) {
        return specieMapper.toDTOList(specieRepositoryJPA.findAllByTimePeriod(timePeriod));
    }
}