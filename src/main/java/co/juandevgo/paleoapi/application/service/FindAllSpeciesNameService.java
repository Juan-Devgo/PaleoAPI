package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesNameUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class FindAllSpeciesNameService implements FindAllSpeciesNameUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesName(String name) {
        return specieMapper.toDTOList(specieRepositoryJPA.findAllByName(name));
    }
}