package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpeciesMapper;
import co.juandevgo.paleoapi.application.ports.in.FindAllSpeciesNameUseCase;
import co.juandevgo.paleoapi.application.ports.out.ISpeciesRepository;
import jakarta.transaction.Transactional;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;

import java.util.List;

@Service
@RequiredArgsConstructor
@Transactional
public class FindAllSpeciesNameService implements FindAllSpeciesNameUseCase {

    private final ISpeciesRepository speciesRepository;

    private final SpeciesMapper speciesMapper;

    @Override
    public List<SpecieDTO> findAllSpeciesName(String name) {
        return speciesMapper.toDTOFromDomainEntityLists(speciesRepository.findAllByName(name));
    }
}