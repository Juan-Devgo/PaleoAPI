package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesLocationUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class FindAllSpeciesLocationService implements FindAllSpeciesLocationUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesLocation(String location) {
        return specieMapper.toDTOList(specieRepositoryJPA.findAllByLocation(location));
    }
}