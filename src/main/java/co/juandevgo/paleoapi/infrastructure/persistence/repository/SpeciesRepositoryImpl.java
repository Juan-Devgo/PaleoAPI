package co.juandevgo.paleoapi.infrastructure.persistence.repository;

import co.juandevgo.paleoapi.application.mapper.SpeciesMapper;
import co.juandevgo.paleoapi.application.ports.out.ISpeciesRepository;
import co.juandevgo.paleoapi.domain.Specie;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Repository;

import java.util.List;

@Repository
@RequiredArgsConstructor
public class SpeciesRepositoryImpl implements ISpeciesRepository {

    private final SpeciesRepositoryJPA speciesRepositoryJPA;

    private final SpeciesMapper speciesMapper;

    @Override
    public void createSpecie(Specie specie) {
        speciesRepositoryJPA.save(speciesMapper.toEntity(specie));
    }

    @Override
    public List<Specie> getAllSpecies() {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAll());
    }

    @Override
    public List<Specie> findAllByName(String name) {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAllByName(name));
    }

    @Override
    public List<Specie> findAllByGroup(String group) {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAllByGroup(group));
    }

    @Override
    public List<Specie> findAllByTimePeriod(String timePeriod) {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAllByTimePeriod(timePeriod));
    }

    @Override
    public List<Specie> findAllByClassification(String classification) {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAllByClassification(classification));
    }

    @Override
    public List<Specie> findAllByLocation(String location) {
        return speciesMapper.toDomainEntityFromEntityLists(speciesRepositoryJPA.findAllByLocation(location));
    }
}