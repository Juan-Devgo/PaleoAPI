package co.juandevgo.paleoapi.infrastructure.controller;

import co.juandevgo.paleoapi.application.dto.CreateSpecieResponseDTO;
import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.ports.in.*;
import lombok.RequiredArgsConstructor;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;

@RestController
@RequestMapping(value = "/api")
@RequiredArgsConstructor
public class SpeciesController implements ISpeciesRestUI {

    private final CreateSpecieUseCase createSpecieUseCase;

    private final GetAllSpeciesUseCase getAllSpeciesUseCase;

    private final FindAllSpeciesNameUseCase findAllSpeciesNameUseCase;

    private final FindAllSpeciesGroupUseCase findAllSpeciesGroupUseCase;

    private final FindAllSpeciesTimePeriodUseCase findAllSpeciesTimePeriodUseCase;

    private final FindAllSpeciesClassificationUseCase findAllSpeciesClassificationUseCase;

    private final FindAllSpeciesLocationUseCase findAllSpeciesLocationUseCase;

    @Override
    public CreateSpecieResponseDTO createSpecie(SpecieDTO specie) {
        return createSpecieUseCase.createSpecie(specie);
    }

    @Override
    public List<SpecieDTO> getAllSpecies() {
        return getAllSpeciesUseCase.getAllSpecies();
    }

    @Override
    public List<SpecieDTO> getSpecieByName(String name) {
        return findAllSpeciesNameUseCase.findAllSpeciesName(name);
    }

    @Override
    public List<SpecieDTO> getSpecieByGroup(String group) {
        return findAllSpeciesGroupUseCase.findAllSpeciesGroup(group);
    }

    @Override
    public List<SpecieDTO> getSpecieByTimePeriod(String timePeriod) {
        return findAllSpeciesTimePeriodUseCase.findAllSpeciesTimePeriod(timePeriod);
    }

    @Override
    public List<SpecieDTO> getSpecieByClassification(String classification) {
        return findAllSpeciesClassificationUseCase.findAllSpeciesClassification(classification);
    }

    @Override
    public List<SpecieDTO> getSpecieByLocation(String location) {
        return findAllSpeciesLocationUseCase.findAllSpeciesLocation(location);
    }
}