package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpeciesMapper;
import co.juandevgo.paleoapi.application.ports.in.GetAllSpeciesUseCase;
import co.juandevgo.paleoapi.application.ports.out.ISpeciesRepository;
import co.juandevgo.paleoapi.infrastructure.persistence.repository.SpeciesRepositoryJPA;
import jakarta.transaction.Transactional;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;

import java.util.List;

@Service
@RequiredArgsConstructor
@Transactional
public class GetAllSpeciesService implements GetAllSpeciesUseCase {

    private final ISpeciesRepository speciesRepository;

    private final SpeciesMapper speciesMapper;

    @Override
    public List<SpecieDTO> getAllSpecies() {
        return speciesMapper.toDTOFromDomainEntityLists(speciesRepository.getAllSpecies());
    }
}