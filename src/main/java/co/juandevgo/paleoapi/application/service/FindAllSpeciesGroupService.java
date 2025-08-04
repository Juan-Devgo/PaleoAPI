package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpecieMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesGroupUseCase;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpecieRepositoryJPA;

import java.util.List;

public class FindAllSpeciesGroupService implements FindAllSpeciesGroupUseCase {

    private SpecieRepositoryJPA specieRepositoryJPA;

    private SpecieMapper specieMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesGroup(String group) {
        return specieMapper.toDTOList(specieRepositoryJPA.findAllByGroup(group));
    }
}
