package co.juandevgo.paleoapi.infrastructure.controller;

import co.juandevgo.paleoapi.application.dto.CreateSpecieResponseDTO;
import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.application.ports.in.*;
import lombok.RequiredArgsConstructor;
import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
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
    public ResponseEntity<CreateSpecieResponseDTO> createSpecie(SpecieDTO specie) {
        return new ResponseEntity<>(createSpecieUseCase.createSpecie(specie), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>> getAllSpecies() {
        return new ResponseEntity<>(getAllSpeciesUseCase.getAllSpecies(), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>> getSpecieByName(String name) {
        return new ResponseEntity<>(findAllSpeciesNameUseCase.findAllSpeciesName(name), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>>  getSpecieByGroup(String group) {
        return new ResponseEntity<>(findAllSpeciesGroupUseCase.findAllSpeciesGroup(group), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>>  getSpecieByTimePeriod(String timePeriod) {
        return new ResponseEntity<>(findAllSpeciesTimePeriodUseCase.findAllSpeciesTimePeriod(timePeriod), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>>  getSpecieByClassification(String classification) {
        return new ResponseEntity<>(findAllSpeciesClassificationUseCase.findAllSpeciesClassification(classification), HttpStatus.OK);
    }

    @Override
    public ResponseEntity<List<SpecieDTO>>  getSpecieByLocation(String location) {
        return new ResponseEntity<>(findAllSpeciesLocationUseCase.findAllSpeciesLocation(location), HttpStatus.OK);
    }
}