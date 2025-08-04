package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesClassificationUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class FindAllSpeciesClassificationService implements FindAllSpeciesClassificationUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesClassification(String classification) {
        return specieMapper.toDTOList(specieRepositoryJPA.findAllByClassification(classification));
    }
}