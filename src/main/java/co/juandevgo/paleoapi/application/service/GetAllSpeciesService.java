package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.GetAllSpeciesUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class GetAllSpeciesService implements GetAllSpeciesUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> getAllSpecies() {
        return specieMapper.toDTOList(specieRepositoryJPA.findAll());
    }
}