package co.juandevgo.paleoapi.application.service;

import co.juandevgo.paleoapi.application.dto.CreateSpecieResponseDTO;
import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.mapper.SpeciesMapper;
import co.juandevgo.paleoapi.application.ports.in.CreateSpecieUseCase;
import co.juandevgo.paleoapi.application.ports.out.ISpeciesRepository;
import jakarta.transaction.Transactional;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;

@Service
@RequiredArgsConstructor
@Transactional
public class CreateSpecieService implements CreateSpecieUseCase {

    private final ISpeciesRepository specieRepository;

    private final SpeciesMapper speciesMapper;

    @Override
    public CreateSpecieResponseDTO createSpecie(SpecieDTO specie) {
        specieRepository.createSpecie(speciesMapper.toDomainEntity(specie));
        return new CreateSpecieResponseDTO("Especie creada con éxito");
    }
}